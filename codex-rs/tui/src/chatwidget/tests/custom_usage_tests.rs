use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn completed_failed_and_interrupted_turns_refresh_usage_once_each() {
    let (mut chat, mut events, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.enable_custom_usage();
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
                        origin: RateLimitRefreshOrigin::Periodic
                    }
                )
            })
            .count();
        assert_eq!(reads, 1);
    }
}

#[tokio::test]
async fn custom_status_uses_cached_limits_without_requesting_another_read() {
    let (mut chat, mut events, _ops) = make_chatwidget_manual(None).await;
    chat.enable_custom_usage();
    set_chatgpt_auth(&mut chat);
    chat.dispatch_command(SlashCommand::Status);
    assert_matches!(events.try_recv(), Ok(AppEvent::InsertHistoryCell(_)));
    assert!(
        !std::iter::from_fn(|| events.try_recv().ok())
            .any(|event| matches!(event, AppEvent::RefreshRateLimits { .. }))
    );
}

#[tokio::test]
async fn custom_usage_views_refresh_before_opening_the_requested_view() {
    use crate::analytics::TokenActivityView;
    for (argument, view) in [
        ("daily", TokenActivityView::Daily),
        ("weekly", TokenActivityView::Weekly),
        ("cumulative", TokenActivityView::Cumulative),
    ] {
        let (mut chat, mut events, _ops) = make_chatwidget_manual(None).await;
        chat.enable_custom_usage();
        set_chatgpt_auth(&mut chat);
        chat.dispatch_command_with_args(SlashCommand::Usage, argument.to_string(), Vec::new());
        assert_matches!(
            events.try_recv(),
            Ok(AppEvent::RefreshRateLimits {
                origin: RateLimitRefreshOrigin::UsageMenu { .. }
            })
        );
        assert_matches!(events.try_recv(), Ok(AppEvent::OpenAnalytics { view: Some(actual) }) if actual == view);
    }
}

#[tokio::test]
async fn installed_feedback_setting_disables_the_feedback_popup() {
    let (mut chat, _events, _ops) = make_chatwidget_manual(None).await;
    chat.config.feedback_enabled = false;
    chat.dispatch_command(SlashCommand::Feedback);
    let popup = render_bottom_popup(&chat, 80);
    assert_chatwidget_snapshot!("feedback_disabled_by_default", popup);
}
