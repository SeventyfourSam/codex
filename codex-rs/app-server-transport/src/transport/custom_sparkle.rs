//! Optional local connection to the Sparkle web gateway.

use super::CHANNEL_CAPACITY;
use super::TransportEvent;
use super::websocket::run_websocket_connection;
use crate::WebSocketCompression;
use codex_core::config::Config;
use futures::StreamExt;
use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_util::sync::CancellationToken;
use tracing::warn;
use uuid::Uuid;

/// Connect to Sparkle without starting another app-server or blocking startup.
/// The merged Codex configuration is read once when this app-server starts.
pub fn start_sparkle_connection(
    config: &Config,
    runtime_kind: &'static str,
    transport_events: mpsc::Sender<TransportEvent>,
    shutdown: CancellationToken,
) -> JoinHandle<()> {
    let port = config
        .config_layer_stack
        .effective_config()
        .get("web_server_port")
        .and_then(|value| value.as_integer())
        .and_then(|value| u16::try_from(value).ok())
        .filter(|port| *port != 0);
    tokio::spawn(async move {
        let Some(port) = port else {
            return;
        };
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let url = format!("ws://{address}/codex/app-server");

        let instance_id = Uuid::now_v7().to_string();
        let process_id = std::process::id().to_string();
        let mut retry = Duration::from_secs(2);
        loop {
            let Ok(mut request) = url.as_str().into_client_request() else {
                warn!("invalid Sparkle WebSocket request");
                return;
            };
            let headers = request.headers_mut();
            let Ok(pid) = process_id.parse() else { return };
            let Ok(instance) = instance_id.parse() else {
                return;
            };
            let Ok(kind) = runtime_kind.parse() else {
                return;
            };
            headers.insert("x-codex-pid", pid);
            headers.insert("x-codex-instance-id", instance);
            headers.insert("x-codex-runtime", kind);

            // Connect directly to loopback. The workspace's tungstenite fork
            // otherwise honors HTTP_PROXY/ALL_PROXY, even for local URLs.
            let connect = async {
                let socket = tokio::net::TcpStream::connect(address).await?;
                tokio_tungstenite::client_async(request, socket)
                    .await
                    .map_err(std::io::Error::other)
            };
            let connected = tokio::select! {
                _ = shutdown.cancelled() => return,
                result = tokio::time::timeout(Duration::from_secs(5), connect) => result,
            };
            if let Ok(Ok((socket, _))) = connected {
                let started = tokio::time::Instant::now();
                let (writer, reader) = socket.split();
                let (connection_events, mut events) = mpsc::channel(CHANNEL_CAPACITY);
                let driver = tokio::spawn(run_websocket_connection(
                    writer,
                    reader,
                    connection_events,
                    WebSocketCompression::Plain,
                ));
                let mut disconnect = None;
                let mut stopping = false;
                loop {
                    tokio::select! {
                        _ = shutdown.cancelled(), if !stopping => {
                            stopping = true;
                            if let Some(token) = &disconnect {
                                CancellationToken::cancel(token);
                            }
                        }
                        event = events.recv() => {
                            let Some(event) = event else { break };
                            if let TransportEvent::ConnectionOpened { disconnect_sender, .. } = &event {
                                disconnect = disconnect_sender.clone();
                                if stopping {
                                    if let Some(token) = &disconnect {
                                        token.cancel();
                                    }
                                }
                            }
                            if !stopping {
                                tokio::select! {
                                    result = transport_events.send(event) => {
                                        if result.is_err() {
                                            stopping = true;
                                            if let Some(token) = &disconnect {
                                                token.cancel();
                                            }
                                        }
                                    }
                                    _ = shutdown.cancelled() => {
                                        stopping = true;
                                        if let Some(token) = &disconnect {
                                            token.cancel();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                let _ = driver.await;
                if stopping {
                    return;
                }
                if started.elapsed() >= Duration::from_secs(30) {
                    retry = Duration::from_secs(2);
                }
            }
            tokio::select! {
                _ = shutdown.cancelled() => return,
                _ = tokio::time::sleep(retry) => {}
            }
            retry = (retry * 2).min(Duration::from_secs(30));
        }
    })
}
