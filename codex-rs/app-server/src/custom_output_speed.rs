//! Time model requests through the upstream extension API, sharing only local memory.

use codex_extension_api::ModelRequestContributor;
use codex_extension_api::ModelRequestInput;
use codex_extension_api::ModelRequestKind;
use codex_extension_api::ModelResponseInterceptor;
use codex_extension_api::ModelResponseStream;
use codex_extension_api::ResponseEvent;
use codex_protocol::ThreadId;
use futures::StreamExt;
use tokio::time::Instant;

#[derive(Debug)]
pub(crate) struct OutputSpeed;

impl ModelRequestContributor for OutputSpeed {
    fn request(&self, input: ModelRequestInput<'_>) -> Option<Box<dyn ModelResponseInterceptor>> {
        if input.kind != ModelRequestKind::Generation {
            return None;
        }
        Some(Box::new(RequestTimer {
            started: Instant::now(),
            thread: ThreadId::from_string(input.thread_id).ok()?,
        }))
    }
}

struct RequestTimer {
    started: Instant,
    thread: ThreadId,
}

impl ModelResponseInterceptor for RequestTimer {
    fn intercept(self: Box<Self>, stream: ModelResponseStream) -> ModelResponseStream {
        let mut pending = Some(self);
        Box::pin(stream.inspect(move |event| match event {
            Ok(ResponseEvent::Completed { token_usage, .. }) => {
                if let Some(timer) = pending.take() {
                    codex_otel::custom_output_speed::record(
                        timer.thread,
                        token_usage.as_ref().map(|usage| usage.output_tokens),
                        timer.started.elapsed(),
                    );
                }
            }
            Err(_) => {
                pending = None;
            }
            _ => {}
        }))
    }
}

#[cfg(test)]
#[path = "custom_output_speed_tests.rs"]
mod tests;
