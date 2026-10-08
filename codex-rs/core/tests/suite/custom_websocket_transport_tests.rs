//! Exercise the custom distribution's WebSocket-only inference policy through real requests.

use anyhow::Result;
use codex_core::TurnInputRequest;
use codex_protocol::protocol::EventMsg;
use codex_protocol::user_input::UserInput;
use core_test_support::responses;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::start_websocket_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use std::time::Duration;
use tokio::time::timeout;
use wiremock::Mock;
use wiremock::ResponseTemplate;
use wiremock::http::Method;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

#[derive(Debug, Default, PartialEq)]
struct ObservedTurn {
    errors: Vec<String>,
    retries: Vec<String>,
    warnings: Vec<String>,
}

async fn observe_turn(test: &TestCodex, prompt: &str) -> Result<ObservedTurn> {
    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: prompt.to_owned(),
            text_elements: Vec::new(),
        }]))
        .await?;
    let mut observed = ObservedTurn::default();
    loop {
        let event = timeout(Duration::from_secs(/*secs*/ 15), test.codex.next_event()).await??;
        match event.msg {
            EventMsg::Error(error) => observed.errors.push(error.message),
            EventMsg::StreamError(error) => observed.retries.push(error.message),
            EventMsg::Warning(warning) => observed.warnings.push(warning.message),
            EventMsg::TurnComplete(_) => return Ok(observed),
            _ => {}
        }
    }
}

async fn inference_methods(server: &wiremock::MockServer) -> Vec<Method> {
    server
        .received_requests()
        .await
        .expect("mock server request log")
        .into_iter()
        .filter(|request| request.url.path().ends_with("/responses"))
        .map(|request| request.method)
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upgrade_required_keeps_websockets_available_for_the_next_turn() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let server = responses::start_mock_server().await;
    Mock::given(method("GET"))
        .and(path_regex(".*/responses$"))
        .respond_with(ResponseTemplate::new(/*s*/ 426))
        .mount(&server)
        .await;
    let mut builder = test_codex().with_config(|config| {
        config.model_provider.supports_websockets = true;
        config.model_provider.stream_max_retries = Some(2);
        config.model_provider.request_max_retries = Some(0);
    });
    let test = builder.build_with_auto_env(&server).await?;

    let first = observe_turn(&test, "first").await?;
    insta::assert_snapshot!(first.errors.join("\n"), @"unsupported operation: HTTP inference is disabled in this custom build. Check the WebSocket connection and send your message again.");
    let first_methods = inference_methods(&server).await;
    assert!(!first_methods.is_empty());
    assert_eq!(first_methods, vec![Method::GET; first_methods.len()]);

    let second = observe_turn(&test, "continue").await?;
    assert_eq!(second.errors, first.errors);
    let second_methods = inference_methods(&server).await;
    assert!(second_methods.len() > first_methods.len());
    assert_eq!(second_methods, vec![Method::GET; second_methods.len()]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exhausted_retries_end_the_turn_without_disabling_websockets() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let server = responses::start_mock_server().await;
    Mock::given(method("GET"))
        .and(path_regex(".*/responses$"))
        .respond_with(ResponseTemplate::new(/*s*/ 503).set_body_string("temporarily unavailable"))
        .mount(&server)
        .await;
    let mut builder = test_codex().with_config(|config| {
        config.model_provider.supports_websockets = true;
        config.model_provider.stream_max_retries = Some(2);
        config.model_provider.request_max_retries = Some(0);
    });
    let test = builder.build_with_auto_env(&server).await?;
    for prompt in ["first", "continue"] {
        let before = inference_methods(&server).await.len();
        let observed = observe_turn(&test, prompt).await?;
        assert_eq!(observed.errors.len(), 1);
        assert!(observed.errors[0].contains("503"));
        assert!(
            observed
                .retries
                .iter()
                .any(|message| message.contains("2/2"))
        );
        let methods = inference_methods(&server).await;
        assert!(methods.len() >= before + 3);
        assert_eq!(methods, vec![Method::GET; methods.len()]);
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_only_provider_is_rejected_before_uploading_context() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let server = responses::start_mock_server().await;
    let test = test_codex().build_with_auto_env(&server).await?;
    let observed = observe_turn(&test, "do not upload this context over HTTP").await?;
    insta::assert_snapshot!(observed.errors.join("\n"), @"unsupported operation: HTTP inference is disabled in this custom build. Check the WebSocket connection and send your message again.");
    assert_eq!(inference_methods(&server).await, Vec::<Method>::new());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disconnected_websocket_recovers_and_continues_the_same_thread() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let server = start_websocket_server(vec![
        vec![vec![ev_response_created("broken")]],
        vec![
            vec![ev_response_created("recovered"), ev_completed("recovered")],
            vec![ev_response_created("next"), ev_completed("next")],
        ],
    ])
    .await;
    let models_server = responses::start_mock_server().await;
    let base_url = format!("{}/v1", server.uri());
    let mut builder = test_codex().with_config(move |config| {
        config.model_provider.base_url = Some(base_url);
        config.model_provider.supports_websockets = true;
        config.model_provider.stream_max_retries = Some(2);
        config.model_provider.request_max_retries = Some(0);
    });
    let test = builder.build_with_auto_env(&models_server).await?;
    let first = observe_turn(&test, "first").await?;
    let second = observe_turn(&test, "continue").await?;
    assert_eq!(
        (first.errors, second.errors),
        (Vec::<String>::new(), Vec::<String>::new())
    );
    assert_eq!(
        server
            .connections()
            .iter()
            .map(Vec::len)
            .collect::<Vec<_>>(),
        vec![1, 2],
    );
    server.shutdown().await;
    Ok(())
}
