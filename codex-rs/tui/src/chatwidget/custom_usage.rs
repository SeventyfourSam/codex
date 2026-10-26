//! Custom usage entry points do not change upstream quota ordering or polling implementations.

use super::*;
use codex_app_server_protocol::TurnStatus;

#[cfg(test)]
#[path = "custom_output_speed_tests.rs"]
mod output_speed_tests;

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
    pub(super) fn custom_context_used_display(&self, used: i64) -> String {
        if self.custom_usage_enabled() {
            format!("Ctx {used}%")
        } else {
            format!("Context {used}% used")
        }
    }

    pub(super) fn custom_weekly_limit_display(
        &self,
        window: &RateLimitWindowDisplay,
        label: &str,
    ) -> Option<String> {
        if !self.custom_usage_enabled() {
            return self.status_line_limit_display(Some(window), label);
        }
        let remaining = (100.0 - window.used_percent).clamp(0.0, 100.0);
        let reset = window
            .resets_at
            .as_deref()
            .and_then(|reset| self.compact_usage_reset(reset));
        Some(match reset {
            Some(reset) => format!("{label} {remaining:.0}% ({reset})"),
            None => format!("{label} {remaining:.0}%"),
        })
    }

    fn compact_usage_reset(&self, reset: &str) -> Option<String> {
        // Upstream has already converted to local time. Use the snapshot's date when
        // its formatter omits a same-day date, even if the UI is rendered after midnight.
        let (time, date) = reset
            .split_once(" on ")
            .map_or((reset, None), |(time, date)| (time, Some(date)));
        let time = chrono::NaiveTime::parse_from_str(time, self.clock_format.time_format()).ok()?;
        let date = match date {
            Some(date) => {
                let (day, month) = date.split_once(' ')?;
                format!("{month}/{day:0>2}")
            }
            None => self
                .rate_limit_snapshots_by_limit_id
                .get("codex")?
                .captured_at
                .format("%b/%d")
                .to_string(),
        };
        Some(format!("{date} {}", time.format("%H:%M")))
    }

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
