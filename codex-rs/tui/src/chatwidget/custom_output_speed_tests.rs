use super::*;
use codex_app_server_protocol::ThreadTokenUsageUpdatedNotification;
use codex_otel::custom_output_speed::record;

#[tokio::test]
async fn custom_output_speed_uses_existing_refresh_and_hides_other_threads_and_daemons() {
    let (mut chat, _sender, _events, _ops) =
        crate::chatwidget::tests::make_chatwidget_manual_with_sender().await;
    let thread = ThreadId::new();
    chat.thread_id = Some(thread);
    chat.local_settings.tui.status_line = Some(vec!["model".into(), "token-output-speed".into()]);
    assert_eq!(
        chat.status_line_value_for_item(StatusLineItem::TokenOutputSpeed),
        None
    );
    record(thread, Some(1234), Duration::from_secs(2));
    // Follow the existing usage-notification path without introducing a custom event.
    let usage: ThreadTokenUsageUpdatedNotification = serde_json::from_value(serde_json::json!({
        "threadId": thread.to_string(), "turnId": "turn", "tokenUsage": {
            "total": {"totalTokens": 2234, "inputTokens": 1000, "cachedInputTokens": 0, "outputTokens": 1234, "reasoningOutputTokens": 500},
            "last": {"totalTokens": 2234, "inputTokens": 1000, "cachedInputTokens": 0, "outputTokens": 1234, "reasoningOutputTokens": 500},
            "modelContextWindow": null
        }
    })).unwrap();
    chat.handle_server_notification(ServerNotification::ThreadTokenUsageUpdated(usage), None);
    // App's thread-event handler refreshes the status line after forwarding usage notifications.
    chat.refresh_status_line();
    let status = chat.bottom_pane.status_line_text().unwrap();
    assert!(status.contains("617.0 tk/s"));
    assert!(status.contains(chat.model_display_name()));
    assert_eq!(
        "token-output-speed".parse(),
        Ok(StatusLineItem::TokenOutputSpeed)
    );

    chat.thread_id = Some(ThreadId::new());
    assert_eq!(
        chat.status_line_value_for_item(StatusLineItem::TokenOutputSpeed),
        None
    );
    chat.thread_id = Some(thread);
    for is_local_daemon in [true, false] {
        chat.remote_connection = Some(RemoteConnectionStatus {
            address: "test".into(),
            version: "test".into(),
            is_local_daemon,
            is_managed_daemon: false,
        });
        assert_eq!(
            chat.status_line_value_for_item(StatusLineItem::TokenOutputSpeed),
            None
        );
    }
    chat.remote_connection = None;
    chat.local_settings.tui.status_line = Some(vec!["model".into()]);
    chat.refresh_status_line();
    assert!(
        !chat
            .bottom_pane
            .status_line_text()
            .unwrap()
            .contains("tk/s")
    );
    record(thread, None, Duration::from_secs(1));
    assert_eq!(
        chat.status_line_value_for_item(StatusLineItem::TokenOutputSpeed),
        None
    );
}
