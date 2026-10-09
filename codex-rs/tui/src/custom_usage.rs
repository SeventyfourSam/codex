//! Network policy for custom usage reads, separate from upstream scheduling/state machines.

use crate::app_event::RateLimitRefreshOrigin;

pub(crate) fn enabled() -> bool {
    std::env::var("CODEX_CUSTOM_USAGE_POLICY").as_deref() != Ok("upstream")
}

pub(crate) fn allow_refresh(origin: RateLimitRefreshOrigin) -> bool {
    !matches!(
        origin,
        RateLimitRefreshOrigin::Recovery | RateLimitRefreshOrigin::StatusCommand { .. }
    )
}

pub(crate) fn exclude_reset_credit_details(origin: RateLimitRefreshOrigin) -> bool {
    if enabled() {
        !matches!(origin, RateLimitRefreshOrigin::ResetPicker { .. })
    } else {
        origin == RateLimitRefreshOrigin::Periodic
    }
}
