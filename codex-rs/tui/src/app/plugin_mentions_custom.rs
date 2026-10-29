//! Mention completion needs installed plugins, never the full remote catalog.

use super::super::App;
use super::super::AppServerSession;
use super::super::background_requests;
use super::*;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::PluginInstalledParams;
use codex_app_server_protocol::PluginInstalledResponse;
use codex_app_server_protocol::RequestId;
use codex_utils_absolute_path::AbsolutePathBuf;

fn enabled() -> bool {
    std::env::var("CODEX_CUSTOM_PLUGIN_CATALOG_POLICY").as_deref() != Ok("upstream")
}

fn installed_request(cwd: PathBuf) -> Result<ClientRequest> {
    Ok(ClientRequest::PluginInstalled {
        request_id: RequestId::String(format!("plugin-mentions-{}", Uuid::new_v4())),
        params: PluginInstalledParams {
            cwds: Some(vec![
                AbsolutePathBuf::try_from(cwd).wrap_err("plugin mention cwd must be absolute")?,
            ]),
            install_suggestion_plugin_names: None,
        },
    })
}

pub(super) async fn request_plugin_list(
    request_handle: AppServerRequestHandle,
    cwd: PathBuf,
) -> Result<PluginListResponse> {
    if !enabled() {
        return background_requests::request_plugin_list(request_handle, cwd).await;
    }
    request_installed_plugin_list(request_handle, cwd).await
}

async fn request_installed_plugin_list(
    request_handle: AppServerRequestHandle,
    cwd: PathBuf,
) -> Result<PluginListResponse> {
    let response: PluginInstalledResponse = request_handle
        .request_typed(installed_request(cwd)?)
        .await
        .wrap_err("plugin/installed failed for TUI mentions")?;
    Ok(PluginListResponse {
        marketplaces: response.marketplaces,
        marketplace_load_errors: response.marketplace_load_errors,
        featured_plugin_ids: Vec::new(),
    })
}

pub(in crate::app) fn refresh_visible_catalog(
    app: &mut App,
    app_server: &AppServerSession,
    cwd: PathBuf,
) {
    // An unsolicited import notification must not open/download the catalog.
    // Explicit /plugins and plugin install/uninstall actions keep their normal path.
    if !enabled() || catalog_visible(app) {
        app.fetch_plugins_list(app_server, cwd);
    }
}

fn catalog_visible(app: &App) -> bool {
    app.chat_widget
        .selected_index_for_present_view("plugins-selection")
        .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_app_server_protocol::JSONRPCMessage;
    use futures::SinkExt;
    use futures::StreamExt;
    use serde_json::json;
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;
    use tokio_tungstenite::tungstenite::Message;

    #[test]
    fn mentions_request_only_installed_plugins_in_the_current_directory() {
        let directory = tempfile::tempdir().unwrap();
        let request = installed_request(directory.path().to_path_buf()).unwrap();
        let value = serde_json::to_value(request).unwrap();
        assert_eq!(value["method"], "plugin/installed");
        assert_eq!(
            value["params"]["cwds"][0],
            directory.path().to_str().unwrap()
        );
        assert!(value["params"]["installSuggestionPluginNames"].is_null());
    }

    #[tokio::test]
    async fn mentions_use_installed_rpc_and_never_fall_back_to_catalog() {
        for unsupported in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let websocket_url = format!("ws://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut ws = accept_async(stream).await.unwrap();
                let mut calls = Vec::new();
                while let Some(frame) = ws.next().await {
                    let Message::Text(text) = frame.unwrap() else {
                        continue;
                    };
                    let JSONRPCMessage::Request(request) = serde_json::from_str(&text).unwrap()
                    else {
                        continue;
                    };
                    let result = match request.method.as_str() {
                        "initialize" => {
                            json!({"id": request.id, "result": {"userAgent": "catalog-test"}})
                        }
                        "plugin/installed" => {
                            calls.push(request.method);
                            if unsupported {
                                json!({"id": request.id, "error": {"code": -32601, "message": "unknown method"}})
                            } else {
                                json!({"id": request.id, "result": {"marketplaces": [], "marketplaceLoadErrors": []}})
                            }
                        }
                        other => panic!("unexpected network request: {other}"),
                    };
                    ws.send(Message::Text(result.to_string().into()))
                        .await
                        .unwrap();
                }
                calls
            });
            let client = crate::connect_remote_app_server(
                codex_app_server_client::RemoteAppServerEndpoint::WebSocket {
                    websocket_url,
                    auth_token: None,
                },
            )
            .await
            .unwrap();
            let directory = tempfile::tempdir().unwrap();
            let result = request_installed_plugin_list(
                client.request_handle(),
                directory.path().to_path_buf(),
            )
            .await;
            assert_eq!(result.is_err(), unsupported);
            if let Ok(response) = result {
                assert!(plugin_mentions_from_list_response(response).is_empty());
            }
            client.shutdown().await.unwrap();
            assert_eq!(server.await.unwrap(), vec!["plugin/installed"]);
        }
    }

    #[tokio::test]
    async fn opening_plugins_allows_catalog_refresh_but_idle_notifications_do_not() {
        let (mut app, mut events, _) = crate::app::tests::make_test_app_with_channels().await;
        while events.try_recv().is_ok() {}
        assert!(!catalog_visible(&app));
        app.chat_widget.add_plugins_output();
        assert!(
            std::iter::from_fn(|| events.try_recv().ok())
                .any(|event| matches!(event, AppEvent::FetchPluginsList { .. }))
        );
        app.chat_widget.on_plugins_loaded(
            app.chat_widget.config_ref().cwd.to_path_buf(),
            Ok(PluginListResponse {
                marketplaces: Vec::new(),
                marketplace_load_errors: Vec::new(),
                featured_plugin_ids: Vec::new(),
            }),
        );
        assert!(catalog_visible(&app));
    }
}
