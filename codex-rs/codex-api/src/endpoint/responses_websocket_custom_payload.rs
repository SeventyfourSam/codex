//! Custom wire-only image compression and bounded WebSocket history replay.
//! Logical history stays unchanged, so upstream continuation matching still works.

use super::*;
use serde::Serialize;
use std::collections::HashSet;

#[path = "responses_websocket_custom_images.rs"]
mod images;

// Conservative client budgets, not a claim about the server's exact limit.
const TARGET_BYTES: usize = 4 * 1024 * 1024;
const MAX_BYTES: usize = 12 * 1024 * 1024;
const CONTINUATION_RESERVE: usize = 1024;

enum Prepared {
    Direct(String),
    Replay {
        fields: JsonMap<String, Value>,
        batches: Vec<Vec<Value>>,
    },
}

fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::InvalidRequest {
        message: message.into(),
    }
}

// Keep routing fields before the potentially large input, like the upstream serializer.
fn encode(fields: &JsonMap<String, Value>, input: &[Value]) -> Result<String, ApiError> {
    #[derive(Serialize)]
    struct Request<'a> {
        #[serde(flatten)]
        fields: &'a JsonMap<String, Value>,
        input: &'a [Value],
    }
    serde_json::to_string(&Request { fields, input })
        .map_err(|error| invalid(format!("failed to encode WebSocket replay: {error}")))
}

fn prepare(text: String, target: usize, maximum: usize) -> Result<Prepared, ApiError> {
    if text.len() <= target && !text.contains("data:image/png;base64,") {
        return Ok(Prepared::Direct(text));
    }
    let mut value: Value = serde_json::from_str(&text)
        .map_err(|error| invalid(format!("invalid WebSocket request: {error}")))?;
    let Some(fields) = value.as_object_mut() else {
        return Ok(Prepared::Direct(text));
    };
    if fields.get("type").and_then(Value::as_str) != Some("response.create") {
        return Ok(Prepared::Direct(text));
    }
    let Some(Value::Array(mut input)) = fields.remove("input") else {
        return Ok(Prepared::Direct(text));
    };
    for item in &mut input {
        images::compress(item);
    }
    let encoded = encode(fields, &input)?;
    if encoded.len() <= target {
        return Ok(Prepared::Direct(encoded));
    }

    let overhead = encode(fields, &[])?.len() + CONTINUATION_RESERVE;
    if overhead >= maximum {
        return Err(invalid(
            "WebSocket request metadata is too large to replay safely",
        ));
    }
    let target_input = target.saturating_sub(overhead);
    let max_input = maximum - overhead;
    let mut batches = Vec::new();
    let mut batch = Vec::new();
    let mut batch_bytes = 0;
    let mut group = Vec::new();
    let mut group_bytes = 0;
    let mut pending = HashSet::new();
    let input_len = input.len();
    for (index, item) in input.into_iter().enumerate() {
        // Never preload a compaction trigger: only the final request may compact.
        let trigger = item.get("type").and_then(Value::as_str) == Some("compaction_trigger");
        if trigger && (!group.is_empty() || index + 1 != input_len) {
            return Err(invalid(
                "compaction must follow complete tool exchanges and be the final input item",
            ));
        }
        let kind = item.get("type").and_then(Value::as_str).unwrap_or_default();
        if let Some(call_id) = item.get("call_id").and_then(Value::as_str) {
            if kind.ends_with("_call") {
                pending.insert(call_id.to_owned());
            } else if kind.ends_with("_call_output") {
                pending.remove(call_id);
            }
        }
        group_bytes += serde_json::to_vec(&item)
            .map_err(|error| invalid(error.to_string()))?
            .len()
            + 1;
        group.push(item);
        if !pending.is_empty() {
            continue;
        }
        if group_bytes > max_input {
            return Err(invalid(format!(
                "A single image or inseparable tool exchange exceeds the WebSocket replay budget ({maximum} bytes); reduce that input"
            )));
        }
        if !batch.is_empty() && batch_bytes + group_bytes > target_input {
            batches.push(std::mem::take(&mut batch));
            batch_bytes = 0;
        }
        batch.append(&mut group);
        batch_bytes += std::mem::take(&mut group_bytes);
    }
    // An incomplete tool exchange may only be present in the final generated request.
    if !group.is_empty() {
        if group_bytes > max_input {
            return Err(invalid(
                "An unfinished tool exchange is too large to replay safely",
            ));
        }
        if !batch.is_empty() && batch_bytes + group_bytes > target_input {
            batches.push(std::mem::take(&mut batch));
        }
        batch.append(&mut group);
    }
    if !batch.is_empty() {
        batches.push(batch);
    }
    if batches.len() < 2 {
        if encoded.len() > maximum {
            return Err(invalid(
                "WebSocket input cannot be split into safe replay batches",
            ));
        }
        return Ok(Prepared::Direct(encoded));
    }
    Ok(Prepared::Replay {
        fields: std::mem::take(fields),
        batches,
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run(
    ws_stream: &mut WsStream,
    tx_event: mpsc::Sender<Result<ResponseEvent, ApiError>>,
    request_text: String,
    idle_timeout: Duration,
    telemetry: Option<Arc<dyn WebsocketTelemetry>>,
    turn_state: Option<&OnceLock<String>>,
    timing_log_context: &ResponsesWebsocketTimingLogContext,
    interrupt: impl Future<Output = ResponseInterrupt> + Clone + Unpin,
    completion: &mut Option<ResponseEvent>,
) -> Result<(), ApiError> {
    let prepared =
        tokio::task::spawn_blocking(move || prepare(request_text, TARGET_BYTES, MAX_BYTES))
            .await
            .map_err(|error| invalid(format!("failed to prepare WebSocket payload: {error}")))??;
    let (mut fields, mut batches) = match prepared {
        Prepared::Direct(text) => {
            return super::run_websocket_response_stream(
                ws_stream,
                tx_event,
                text,
                idle_timeout,
                telemetry,
                turn_state,
                timing_log_context,
                interrupt,
                completion,
            )
            .await;
        }
        Prepared::Replay { fields, batches } => (fields, batches),
    };
    let final_input = batches
        .pop()
        .ok_or_else(|| invalid("empty WebSocket replay plan"))?;
    let original_generate = fields.insert("generate".into(), Value::Bool(false));
    for input in batches {
        if interrupt.clone().now_or_never().is_some() {
            return Err(ApiError::Stream(
                "WebSocket history replay interrupted".into(),
            ));
        }
        let text = encode(&fields, &input)?;
        if text.len() > MAX_BYTES {
            return Err(invalid("WebSocket replay batch exceeds its byte budget"));
        }
        let (tx_batch, mut rx_batch) = mpsc::channel(32);
        let mut batch_completion = None;
        let replay = super::run_websocket_response_stream(
            ws_stream,
            tx_batch,
            text,
            idle_timeout,
            telemetry.clone(),
            turn_state,
            timing_log_context,
            interrupt.clone(),
            &mut batch_completion,
        );
        // Do not expose internal response IDs or repeated input-token usage to the turn.
        // Drain concurrently to retain upstream metadata, error, timeout and cancellation handling.
        let collect = async {
            let mut response_id = None;
            while let Some(event) = rx_batch.recv().await {
                match event? {
                    ResponseEvent::Created { .. } => {}
                    ResponseEvent::Completed {
                        response_id: id, ..
                    } => response_id = Some(id),
                    ResponseEvent::OutputItemAdded(_)
                    | ResponseEvent::OutputItemDone(_)
                    | ResponseEvent::OutputTextDelta(_)
                    | ResponseEvent::ToolCallInputDelta { .. }
                    | ResponseEvent::ReasoningSummaryDelta { .. }
                    | ResponseEvent::ReasoningContentDelta { .. } => {
                        return Err(invalid(
                            "Server generated output during WebSocket history preloading",
                        ));
                    }
                    event => tx_event
                        .send(Ok(event))
                        .await
                        .map_err(|_| ApiError::Stream("response event consumer dropped".into()))?,
                }
            }
            Ok(response_id)
        };
        let (_, response_id) = tokio::try_join!(replay, collect)?;
        if interrupt.clone().now_or_never().is_some() {
            return Err(ApiError::Stream(
                "WebSocket history replay interrupted".into(),
            ));
        }
        let response_id = response_id.filter(|id| !id.is_empty()).ok_or_else(|| {
            ApiError::Stream("WebSocket history preloading ended without response.completed".into())
        })?;
        fields.insert("previous_response_id".into(), Value::String(response_id));
    }
    if let Some(generate) = original_generate {
        fields.insert("generate".into(), generate);
    } else {
        fields.remove("generate");
    }
    let final_text = encode(&fields, &final_input)?;
    if final_text.len() > MAX_BYTES {
        return Err(invalid(
            "Final WebSocket replay batch exceeds its byte budget",
        ));
    }
    super::run_websocket_response_stream(
        ws_stream,
        tx_event,
        final_text,
        idle_timeout,
        telemetry,
        turn_state,
        timing_log_context,
        interrupt,
        completion,
    )
    .await
}
