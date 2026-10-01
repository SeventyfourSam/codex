use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn completed_failed_and_interrupted_turns_refresh_usage_once_each() {
    let (mut chat, mut events, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    set_chatgpt_auth(&mut chat);
    for status in [
        AppServerTurnStatus::Completed,
        AppServerTurnStatus::Failed,
        AppServerTurnStatus::Interrupted,
    ] {
        let turn_id = format!("turn-{status:?}");
        let notification = ServerNotification::TurnCompleted(TurnCompletedNotification {
            thread_id: String::new(),
            turn: app_server_turn(
                &turn_id, status, /*duration_ms*/ None, /*error*/ None,
            ),
        });
        chat.handle_server_notification(notification.clone(), Some(ReplayKind::ThreadSnapshot));
        chat.handle_server_notification(notification.clone(), /*replay_kind*/ None);
        chat.handle_server_notification(notification, /*replay_kind*/ None);
        let reads = std::iter::from_fn(|| events.try_recv().ok())
            .filter(|event| {
                matches!(
                    event,
                    AppEvent::RefreshRateLimits {
                        origin: RateLimitRefreshOrigin::TurnCompleted
                    }
                )
            })
            .count();
        assert_eq!(reads, 1);
    }
}
