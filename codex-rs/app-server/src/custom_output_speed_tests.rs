use super::*;
use codex_extension_api::ModelResponseError;
use codex_otel::custom_output_speed::display;
use codex_protocol::protocol::TokenUsage;
use std::time::Duration;
use tokio::sync::mpsc;

fn request(thread: ThreadId, kind: ModelRequestKind) -> Option<Box<dyn ModelResponseInterceptor>> {
    OutputSpeed.request(ModelRequestInput {
        kind,
        thread_id: &thread.to_string(),
        model: "model",
        client_metadata: &mut None,
    })
}

fn completed(tokens: Option<i64>) -> Result<ResponseEvent, ModelResponseError> {
    Ok(ResponseEvent::Completed {
        response_id: "response".into(),
        token_usage: tokens.map(|output_tokens| TokenUsage {
            output_tokens,
            reasoning_output_tokens: output_tokens / 2,
            ..Default::default()
        }),
        usage_metadata: None,
        end_turn: Some(true),
    })
}

#[tokio::test(start_paused = true)]
async fn custom_output_speed_excludes_tools_and_counts_real_usage_including_reasoning_once() {
    let thread = ThreadId::new();
    let timer = request(thread, ModelRequestKind::Generation).unwrap();
    tokio::time::advance(Duration::from_secs(1)).await;
    let mut stream = timer.intercept(Box::pin(futures::stream::iter([
        Ok(ResponseEvent::OutputTextDelta("tiny visible answer".into())),
        completed(Some(1234)),
        completed(Some(9999)),
    ])));
    assert!(
        matches!(stream.next().await, Some(Ok(ResponseEvent::OutputTextDelta(text))) if text == "tiny visible answer")
    );
    assert_eq!(display(Some(thread), true), None);
    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(matches!(
        stream.next().await,
        Some(Ok(ResponseEvent::Completed { .. }))
    ));
    assert_eq!(display(Some(thread), true).as_deref(), Some("617.0 tk/s"));
    assert!(stream.next().await.is_some());
    assert_eq!(display(Some(thread), true).as_deref(), Some("617.0 tk/s"));
    tokio::time::advance(Duration::from_secs(60)).await; // tool / approval wait
    let timer = request(thread, ModelRequestKind::Generation).unwrap();
    tokio::time::advance(Duration::from_secs(2)).await;
    let mut stream = timer.intercept(Box::pin(futures::stream::iter([completed(Some(100))])));
    assert!(stream.next().await.is_some());
    assert_eq!(display(Some(thread), true).as_deref(), Some("50.0 tk/s"));
}

#[tokio::test(start_paused = true)]
async fn custom_output_speed_preserves_errors_cancellation_and_missing_usage() {
    let thread = ThreadId::new();
    assert!(request(thread, ModelRequestKind::Warmup).is_none());
    let timer = request(thread, ModelRequestKind::Generation).unwrap();
    let mut stream = timer.intercept(Box::pin(futures::stream::iter([
        Err(ModelResponseError::Stream("disconnected".into())),
        completed(Some(100)),
    ])));
    assert!(
        matches!(stream.next().await, Some(Err(ModelResponseError::Stream(message))) if message == "disconnected")
    );
    assert!(stream.next().await.is_some());
    assert_eq!(display(Some(thread), true), None);

    let (tx, rx) = mpsc::channel::<Result<ResponseEvent, ModelResponseError>>(1);
    let upstream = futures::stream::unfold(rx, |mut rx| async {
        rx.recv().await.map(|event| (event, rx))
    });
    let timer = request(thread, ModelRequestKind::Generation).unwrap();
    let stream = timer.intercept(Box::pin(upstream));
    drop(stream);
    assert!(tx.is_closed());
    assert_eq!(display(Some(thread), true), None);

    codex_otel::custom_output_speed::record(thread, Some(100), Duration::from_secs(1));
    let timer = request(thread, ModelRequestKind::Generation).unwrap();
    tokio::time::advance(Duration::from_secs(1)).await;
    let mut stream = timer.intercept(Box::pin(futures::stream::iter([completed(None)])));
    assert!(stream.next().await.is_some());
    assert_eq!(display(Some(thread), true), None);
}
