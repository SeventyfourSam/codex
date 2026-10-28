use super::*;
use base64::Engine;
use image::ImageEncoder;
use image::codecs::png::CompressionType;
use image::codecs::png::FilterType;
use image::codecs::png::PngEncoder;
use serde_json::json;
use tokio::net::TcpListener;

#[test]
#[ignore = "requires CODEX_TEST_PNG_DIR with local PNG fixtures"]
fn external_png_fixture_fidelity_and_compression() {
    let root = std::path::PathBuf::from(std::env::var_os("CODEX_TEST_PNG_DIR").unwrap());
    let output = std::env::var_os("CODEX_TEST_WEBP_DIR").map(std::path::PathBuf::from);
    let mut before = 0usize;
    let mut after = 0usize;
    let started = std::time::Instant::now();
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|extension| extension != "png") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let original = image::load_from_memory(&bytes).unwrap();
        let mut value = image_value(&bytes);
        images::compress(&mut value);
        let uri = value["image_url"].as_str().unwrap();
        let compressed = base64::engine::general_purpose::STANDARD
            .decode(uri.split_once(',').unwrap().1)
            .unwrap();
        let decoded = image::load_from_memory(&compressed).unwrap();
        assert_eq!(original.to_rgba8(), decoded.to_rgba8());
        before += bytes.len();
        after += compressed.len();
        if let Some(output) = &output {
            std::fs::write(
                output.join(path.file_name().unwrap()).with_extension(
                    if uri.starts_with("data:image/webp") {
                        "webp"
                    } else {
                        "png"
                    },
                ),
                &compressed,
            )
            .unwrap();
        }
        println!(
            "{}: {} -> {} bytes",
            path.file_name().unwrap().to_string_lossy(),
            bytes.len(),
            compressed.len()
        );
    }
    assert!(before > 0);
    println!(
        "total_bytes: {before} -> {after}; elapsed: {:?}",
        started.elapsed()
    );
}

fn request(input: Vec<Value>) -> String {
    json!({"type":"response.create","model":"test","store":false,
        "previous_response_id":"existing-response","input":input})
    .to_string()
}

fn text(size: usize) -> Value {
    json!({"type":"message","role":"user","content":[{"type":"input_text","text":"x".repeat(size)}]})
}

#[test]
fn small_request_is_byte_for_byte_unchanged() {
    let original = request(vec![text(20)]);
    let Prepared::Direct(actual) = prepare(original.clone(), TARGET_BYTES, MAX_BYTES).unwrap()
    else {
        panic!("unexpected replay")
    };
    assert_eq!(original, actual);
}

#[test]
fn batches_preserve_order_tool_pairs_and_final_compaction() {
    let mut input = vec![text(1500), text(1500)];
    input.extend([
        json!({"type":"custom_tool_call","call_id":"a","name":"test","input":"a"}),
        json!({"type":"function_call","call_id":"b","name":"test","arguments":"{}"}),
        json!({"type":"custom_tool_call_output","call_id":"a","output":"x".repeat(1700)}),
        json!({"type":"function_call_output","call_id":"b","output":"y".repeat(1700)}),
        text(1500),
        json!({"type":"compaction_trigger"}),
    ]);
    let Prepared::Replay { fields, batches } =
        prepare(request(input.clone()), 4000, 10000).unwrap()
    else {
        panic!("expected replay")
    };
    assert_eq!(fields["previous_response_id"], "existing-response");
    assert_eq!(batches.concat(), input);
    for batch in &batches {
        let calls = batch
            .iter()
            .filter_map(|x| x.get("call_id").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert!(calls.is_empty() || calls == ["a", "b", "a", "b"]);
        let wire = encode(&fields, batch).unwrap();
        assert!(wire.find("\"model\"").unwrap() < wire.find("\"input\"").unwrap());
        assert!(wire.len() < 10000);
    }
    assert_eq!(
        batches.last().unwrap().last().unwrap()["type"],
        "compaction_trigger"
    );
    for batch in &batches[..batches.len() - 1] {
        assert!(batch.iter().all(|x| x["type"] != "compaction_trigger"));
    }
}

#[test]
fn oversize_atomic_item_is_a_nonretryable_error() {
    assert!(matches!(
        prepare(request(vec![text(12000)]), 4000, 10000),
        Err(ApiError::InvalidRequest { .. })
    ));
}

#[test]
fn single_large_atomic_item_can_exceed_target_but_not_maximum() {
    assert!(matches!(
        prepare(request(vec![text(5000)]), 4000, 10000),
        Ok(Prepared::Direct(_))
    ));
}

#[test]
fn trigger_cannot_be_preloaded() {
    assert!(matches!(
        prepare(
            request(vec![json!({"type":"compaction_trigger"}), text(8000)]),
            4000,
            10000
        ),
        Err(ApiError::InvalidRequest { .. })
    ));
}

fn png(alpha: bool) -> (Vec<u8>, Vec<u8>) {
    let channels = if alpha { 4 } else { 3 };
    let mut pixels = Vec::new();
    for y in 0..256u32 {
        for x in 0..256u32 {
            pixels.extend([x as u8, y as u8, (x ^ y) as u8]);
            if alpha {
                pixels.push((x % 3 * 127) as u8);
            }
        }
    }
    let mut bytes = Vec::new();
    PngEncoder::new_with_quality(
        &mut bytes,
        CompressionType::Uncompressed,
        FilterType::NoFilter,
    )
    .write_image(
        &pixels,
        256,
        256,
        if channels == 4 {
            image::ExtendedColorType::Rgba8
        } else {
            image::ExtendedColorType::Rgb8
        },
    )
    .unwrap();
    (bytes, pixels)
}

fn image_value(bytes: &[u8]) -> Value {
    json!({"type":"input_image","image_url":format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(bytes)),"detail":"original"})
}

#[test]
fn lossless_conversion_preserves_rgb_rgba_and_transparent_pixel_values() {
    for alpha in [false, true] {
        let (bytes, pixels) = png(alpha);
        let original = image_value(&bytes);
        let mut converted = original.clone();
        images::compress(&mut converted);
        let uri = converted["image_url"].as_str().unwrap();
        assert!(uri.starts_with("data:image/webp;base64,"));
        assert!(uri.len() < original["image_url"].as_str().unwrap().len());
        let result = base64::engine::general_purpose::STANDARD
            .decode(uri.split_once(',').unwrap().1)
            .unwrap();
        let decoded = image::load_from_memory(&result).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (256, 256));
        let actual = if alpha {
            decoded.to_rgba8().into_raw()
        } else {
            decoded.to_rgb8().into_raw()
        };
        assert_eq!(pixels, actual);
        assert_eq!(converted["detail"], "original");
        let mut cached = original.clone();
        images::compress(&mut cached);
        assert_eq!(cached, converted);
        assert!(
            original["image_url"]
                .as_str()
                .unwrap()
                .starts_with("data:image/png")
        );
    }
}

#[test]
fn metadata_higher_depth_bad_images_and_text_are_not_rewritten() {
    let (mut profile, _) = png(false);
    // Presence alone must prevent a conversion, before decoding the chunk.
    profile.splice(
        33..33,
        [
            0, 0, 0, 4, b'g', b'A', b'M', b'A', 0, 0, 177, 143, 0, 0, 0, 0,
        ],
    );
    let (mut sixteen, _) = png(false);
    sixteen[24] = 16;
    for bytes in [profile, sixteen, vec![0; 70000]] {
        let original = image_value(&bytes);
        let mut actual = original.clone();
        images::compress(&mut actual);
        assert_eq!(actual, original);
    }
    let original = json!({"type":"input_text","text":image_value(&png(false).0)["image_url"]});
    let mut actual = original.clone();
    images::compress(&mut actual);
    assert_eq!(actual, original);
}

type Server = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;

async fn pair() -> (WsStream, Server) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/responses", listener.local_addr().unwrap());
    let factory = HttpClientFactory::new(codex_http_client::OutboundProxyPolicy::ReqwestDefault);
    let connector = WebSocketConnector::new_with_tls_mode(
        &factory,
        codex_websocket_client::WebSocketTlsMode::TungsteniteDefault,
    )
    .unwrap();
    let client = connector.connect_loopback_direct(
        url.into_client_request().unwrap(),
        WebSocketConfig::default(),
    );
    let server = async {
        tokio_tungstenite::accept_async(listener.accept().await.unwrap().0)
            .await
            .unwrap()
    };
    let (client, server) = tokio::join!(client, server);
    (WsStream::new(client.unwrap().0), server)
}

fn context() -> ResponsesWebsocketTimingLogContext {
    ResponsesWebsocketTimingLogContext {
        model: "test".into(),
        session_id: None,
        thread_id: None,
        turn_id: None,
        traceparent: None,
        previous_response_id: None,
        request_start_ms: None,
        warmup: false,
        connection_reused: false,
    }
}

async fn receive(server: &mut Server) -> Value {
    let wire = tokio::time::timeout(Duration::from_secs(10), server.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .into_text()
        .unwrap();
    assert!(wire.len() <= MAX_BYTES);
    serde_json::from_str(&wire).unwrap()
}

async fn send(server: &mut Server, value: Value) {
    server
        .send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
}

async fn complete(server: &mut Server, id: &str, tokens: i64) {
    send(
        server,
        json!({"type":"response.created","response":{"id":id}}),
    )
    .await;
    send(server,json!({"type":"response.completed","response":{"id":id,"usage":{"input_tokens":tokens,"output_tokens":0,"total_tokens":tokens}}})).await;
}

#[tokio::test]
async fn replay_chains_ids_exposes_only_final_usage_and_keeps_incremental_requests_small() {
    let (mut client, mut server) = pair().await;
    let mut history = vec![text(1_100_000); 4];
    history.push(json!({"type":"compaction_trigger"}));
    let original = history.clone();
    let fixture = tokio::spawn(async move {
        let first = receive(&mut server).await;
        assert_eq!(first["generate"], false);
        assert!(
            first["input"]
                .as_array()
                .unwrap()
                .iter()
                .all(|item| item["type"] != "compaction_trigger")
        );
        assert_eq!(first["previous_response_id"], "existing-response");
        complete(&mut server, "batch-1", 999).await;
        let final_request = receive(&mut server).await;
        assert!(final_request.get("generate").is_none());
        assert_eq!(final_request["previous_response_id"], "batch-1");
        assert_eq!(
            final_request["input"].as_array().unwrap().last().unwrap()["type"],
            "compaction_trigger"
        );
        let mut all = first["input"].as_array().unwrap().clone();
        all.extend(final_request["input"].as_array().unwrap().clone());
        assert_eq!(all, original);
        complete(&mut server, "final", 50).await;
        let next = receive(&mut server).await;
        assert_eq!(next["input"].as_array().unwrap().len(), 1);
        assert!(next.get("generate").is_none());
        complete(&mut server, "next", 52).await;
    });
    let (tx, mut rx) = mpsc::channel(32);
    let mut completion = None;
    let interrupt = futures::future::pending::<ResponseInterrupt>().shared();
    run(
        &mut client,
        tx,
        request(history),
        Duration::from_secs(10),
        None,
        None,
        &context(),
        interrupt.clone(),
        &mut completion,
    )
    .await
    .unwrap();
    let mut completions = Vec::new();
    let mut created = Vec::new();
    while let Some(event) = rx.recv().await {
        match event.unwrap() {
            ResponseEvent::Completed {
                response_id,
                token_usage,
                ..
            } => completions.push((response_id, token_usage.unwrap().input_tokens)),
            ResponseEvent::Created { response_id } => created.push(response_id),
            _ => {}
        }
    }
    assert_eq!(completions, vec![("final".into(), 50)]);
    assert_eq!(created, vec![Some("final".into())]);
    let (tx, _rx) = mpsc::channel(32);
    run(
        &mut client,
        tx,
        request(vec![text(10)]),
        Duration::from_secs(10),
        None,
        None,
        &context(),
        interrupt,
        &mut completion,
    )
    .await
    .unwrap();
    fixture.await.unwrap();
}

#[tokio::test]
async fn failed_preload_never_sends_final_request() {
    let (mut client, mut server) = pair().await;
    let fixture = tokio::spawn(async move {
        assert_eq!(receive(&mut server).await["generate"], false);
        send(&mut server,json!({"type":"error","status":400,"error":{"message":"fixture rejection","code":"invalid_request_error"}})).await;
        assert!(
            tokio::time::timeout(Duration::from_millis(100), server.next())
                .await
                .is_err()
        );
    });
    let (tx, mut rx) = mpsc::channel(32);
    let mut completion = None;
    let result = run(
        &mut client,
        tx,
        request(vec![text(1_100_000); 4]),
        Duration::from_secs(10),
        None,
        None,
        &context(),
        futures::future::pending().shared(),
        &mut completion,
    )
    .await;
    assert!(result.is_err());
    assert!(completion.is_none());
    assert!(rx.recv().await.is_none());
    fixture.await.unwrap();
}

#[tokio::test]
async fn cancellation_during_preload_stops_replay_and_discards_internal_usage() {
    let (mut client, mut server) = pair().await;
    let (cancel_tx, cancel_rx) = oneshot::channel();
    let fixture = tokio::spawn(async move {
        receive(&mut server).await;
        send(
            &mut server,
            json!({"type":"response.created","response":{"id":"warmup"}}),
        )
        .await;
        cancel_tx
            .send(ResponseInterrupt::Cancel {
                deadline: Instant::now() + Duration::from_secs(5),
            })
            .unwrap_or_else(|_| panic!("cancel receiver"));
        let interruption = receive(&mut server).await;
        assert_eq!(interruption["type"], "response.interrupt");
        send(
            &mut server,
            json!({"type":"response.interrupt.accepted","response_id":"warmup"}),
        )
        .await;
        send(
            &mut server,
            json!({"type":"response.completed","response":{"id":"warmup"}}),
        )
        .await;
        assert!(
            tokio::time::timeout(Duration::from_millis(100), server.next())
                .await
                .is_err()
        );
    });
    let interrupt = async move { cancel_rx.await.unwrap() }.boxed().shared();
    let (tx, mut rx) = mpsc::channel(32);
    let mut completion = None;
    let result = run(
        &mut client,
        tx,
        request(vec![text(1_100_000); 4]),
        Duration::from_secs(10),
        None,
        None,
        &context(),
        interrupt,
        &mut completion,
    )
    .await;
    assert!(result.is_err());
    assert!(completion.is_none());
    assert!(rx.recv().await.is_none());
    fixture.await.unwrap();
}
