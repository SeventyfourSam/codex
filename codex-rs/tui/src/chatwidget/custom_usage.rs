//! Custom usage entry points do not change upstream quota ordering or polling implementations.

use super::*;
use codex_app_server_protocol::TurnStatus;

#[derive(Debug)]
pub(super) struct CustomUsageState {
    pub(super) enabled: bool,
    pub(super) last_refreshed_turn: Option<String>,
}

impl Default for CustomUsageState {
    fn default() -> Self {
        Self {
            // Compatibility runs can select upstream policy for every new widget,
            // including widgets replaced after bootstrap; installed builds default to custom.
            enabled: crate::custom_usage::enabled(),
            last_refreshed_turn: None,
        }
    }
}

impl ChatWidget {
    pub(crate) fn custom_usage_enabled(&self) -> bool {
        self.turn_lifecycle.custom_usage.enabled
    }

    #[cfg(test)]
    pub(super) fn enable_custom_usage(&mut self) {
        self.turn_lifecycle.custom_usage.enabled = true;
    }

    pub(super) fn open_custom_usage_analytics(
        &mut self,
        view: crate::analytics::TokenActivityView,
    ) {
        if self.custom_usage_enabled() {
            let request_id = self.take_next_rate_limit_reset_request_id();
            self.pending_usage_menu_rate_limit_request_id = Some(request_id);
            self.app_event_tx.send(AppEvent::RefreshRateLimits {
                origin: RateLimitRefreshOrigin::UsageMenu { request_id },
            });
        }
        self.app_event_tx
            .send(AppEvent::OpenAnalytics { view: Some(view) });
    }

    pub(super) fn refresh_custom_usage_after_turn(
        &mut self,
        turn_id: &str,
        status: TurnStatus,
        replayed: bool,
    ) {
        if self.custom_usage_enabled()
            && !replayed
            && status != TurnStatus::InProgress
            && self.should_prefetch_rate_limits()
            && self
                .turn_lifecycle
                .custom_usage
                .last_refreshed_turn
                .as_deref()
                != Some(turn_id)
        {
            self.turn_lifecycle.custom_usage.last_refreshed_turn = Some(turn_id.to_owned());
            self.app_event_tx.send(AppEvent::RefreshRateLimits {
                origin: RateLimitRefreshOrigin::Periodic,
            });
        }
    }
}
