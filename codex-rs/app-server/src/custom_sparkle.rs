//! Add a second, ordinary app-server protocol connection to an embedded server.

use crate::connection_cleanup::ConnectionCleanupTasks;
use crate::message_processor::MessageProcessor;
use crate::outgoing_message::ConnectionId;
use crate::outgoing_message::OutgoingEnvelope;
use crate::outgoing_message::QueuedOutgoingMessage;
use crate::transport::AppServerTransport;
use crate::transport::ConnectionState;
use crate::transport::OutboundConnectionState;
use crate::transport::TransportEvent;
use crate::transport::route_outgoing_envelope;
use codex_app_server_protocol::JSONRPCMessage;
use std::collections::HashMap;
use std::collections::HashSet;
use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tracing::warn;

pub(crate) enum OutboundControl {
    Opened {
        connection_id: ConnectionId,
        state: OutboundConnectionState,
    },
    Closed(ConnectionId),
}

pub(crate) enum IncomingReply {
    Embedded(QueuedOutgoingMessage),
    External(OutgoingEnvelope),
}

pub(crate) fn classify_reply(reply: OutgoingEnvelope, embedded_id: ConnectionId) -> IncomingReply {
    match reply {
        OutgoingEnvelope::ToConnection {
            connection_id,
            message,
            write_complete_tx,
        } if connection_id == embedded_id => IncomingReply::Embedded(QueuedOutgoingMessage {
            message,
            write_complete_tx,
        }),
        reply @ OutgoingEnvelope::ToConnection { .. } => IncomingReply::External(reply),
        OutgoingEnvelope::Broadcast { .. } => unreachable!("replies are connection scoped"),
    }
}

/// Keep the embedded connection's existing ID distinct from transport IDs.
fn external_id(id: ConnectionId) -> ConnectionId {
    ConnectionId(id.0 | (1 << 63))
}

pub(crate) async fn run_outbound_router(
    mut outgoing_rx: mpsc::Receiver<OutgoingEnvelope>,
    mut outbound_connections: HashMap<ConnectionId, OutboundConnectionState>,
    mut control_rx: mpsc::Receiver<OutboundControl>,
    mut shutdown_rx: oneshot::Receiver<()>,
) {
    loop {
        tokio::select! {
            biased;
            _ = &mut shutdown_rx => break,
            event = control_rx.recv() => match event {
                Some(OutboundControl::Opened { connection_id, state }) => {
                    outbound_connections.insert(connection_id, state);
                }
                Some(OutboundControl::Closed(connection_id)) => {
                    outbound_connections.remove(&connection_id);
                }
                None => break,
            },
            envelope = outgoing_rx.recv() => {
                let Some(envelope) = envelope else { break };
                route_outgoing_envelope(&mut outbound_connections, envelope).await;
            }
        }
    }
    for state in outbound_connections.values() {
        state.request_disconnect();
    }
}

pub(crate) async fn handle_event(
    event: TransportEvent,
    processor: &Arc<MessageProcessor>,
    connections: &mut HashMap<ConnectionId, ConnectionState>,
    outbound: &mpsc::Sender<OutboundControl>,
    cleanup: &mut ConnectionCleanupTasks,
) {
    match event {
        TransportEvent::DaemonShutdown => {}
        TransportEvent::ConnectionOpened {
            connection_id,
            origin,
            auth,
            writer,
            disconnect_sender,
        } => {
            let connection_id = external_id(connection_id);
            let initialized = Arc::new(AtomicBool::new(false));
            let experimental = Arc::new(AtomicBool::new(false));
            let opted_out = Arc::new(RwLock::new(HashSet::new()));
            let state = ConnectionState::new(
                origin,
                auth,
                Arc::clone(&initialized),
                Arc::clone(&experimental),
                Arc::clone(&opted_out),
            );
            let output = OutboundConnectionState::new(
                writer,
                initialized,
                experimental,
                opted_out,
                disconnect_sender.clone(),
            );
            if outbound
                .send(OutboundControl::Opened {
                    connection_id,
                    state: output,
                })
                .await
                .is_err()
            {
                if let Some(disconnect) = disconnect_sender {
                    disconnect.cancel();
                }
                return;
            }
            connections.insert(connection_id, state);
        }
        TransportEvent::ConnectionClosed { connection_id } => {
            let connection_id = external_id(connection_id);
            let Some(state) = connections.remove(&connection_id) else {
                return;
            };
            state.session.rpc_gate.close().await;
            let _ = outbound.send(OutboundControl::Closed(connection_id)).await;
            let processor = Arc::clone(processor);
            cleanup.spawn(async move {
                processor
                    .connection_closed(connection_id, &state.session)
                    .await;
            });
        }
        TransportEvent::IncomingMessage {
            connection_id,
            message,
        } => {
            let connection_id = external_id(connection_id);
            let Some(state) = connections.get_mut(&connection_id) else {
                return;
            };
            if state.session.rpc_gate.is_closed() {
                return;
            }
            match message {
                JSONRPCMessage::Request(request) => {
                    let was_initialized = state.session.initialized();
                    let transport = AppServerTransport::WebSocket {
                        bind_address: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
                    };
                    processor
                        .process_request(
                            connection_id,
                            request,
                            &transport,
                            Arc::clone(&state.session),
                        )
                        .await;
                    if let Ok(mut opted_out) = state.outbound_opted_out_notification_methods.write()
                    {
                        *opted_out = state.session.opted_out_notification_methods();
                    } else {
                        warn!("failed to update Sparkle notification preferences");
                    }
                    state
                        .outbound_experimental_api_enabled
                        .store(state.session.experimental_api_enabled(), Ordering::Release);
                    if !was_initialized && state.session.initialized() {
                        processor
                            .send_initialize_notifications_to_connection(connection_id)
                            .await;
                        processor
                            .connection_initialized(
                                connection_id,
                                state.session.request_attestation(),
                            )
                            .await;
                        state.outbound_initialized.store(true, Ordering::Release);
                    }
                }
                JSONRPCMessage::Response(response) => {
                    processor.process_response(connection_id, response).await;
                }
                JSONRPCMessage::Notification(notification) => {
                    processor.process_notification(notification).await;
                }
                JSONRPCMessage::Error(error) => {
                    processor.process_error(connection_id, error).await;
                }
            }
        }
    }
}

pub(crate) async fn close_connections(
    processor: &Arc<MessageProcessor>,
    connections: &mut HashMap<ConnectionId, ConnectionState>,
    outbound: &mpsc::Sender<OutboundControl>,
    cleanup: &mut ConnectionCleanupTasks,
) {
    for (connection_id, state) in connections.drain() {
        state.session.rpc_gate.close().await;
        let _ = outbound.send(OutboundControl::Closed(connection_id)).await;
        let processor = Arc::clone(processor);
        cleanup.spawn(async move {
            processor
                .connection_closed(connection_id, &state.session)
                .await;
        });
    }
    cleanup.drain().await;
}
