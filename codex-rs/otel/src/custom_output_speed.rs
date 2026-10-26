//! Process-local completed-request rates. Never exported, persisted, or sent over RPC.

use codex_protocol::ThreadId;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

const MAX_THREADS: usize = 128;
static RATES: Mutex<LatestRates> = Mutex::new(LatestRates(VecDeque::new()));

#[derive(Default)]
struct LatestRates(VecDeque<(ThreadId, f64)>);

impl LatestRates {
    fn record(&mut self, thread: ThreadId, tokens: Option<i64>, elapsed: Duration) {
        self.0.retain(|(id, _)| *id != thread);
        let Some(tokens) = tokens.filter(|tokens| *tokens >= 0) else {
            return;
        };
        if elapsed.is_zero() {
            return;
        }
        self.0
            .push_back((thread, tokens as f64 / elapsed.as_secs_f64()));
        if self.0.len() > MAX_THREADS {
            self.0.pop_front();
        }
    }

    fn get(&self, thread: ThreadId) -> Option<f64> {
        self.0
            .iter()
            .find(|(id, _)| *id == thread)
            .map(|(_, rate)| *rate)
    }
}

/// Record authoritative output tokens once per completed request, including reasoning once.
pub fn record(thread: ThreadId, tokens: Option<i64>, elapsed: Duration) {
    RATES
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .record(thread, tokens, elapsed);
}

/// Only embedded clients share this process's samples; daemon/remote clients must omit them.
pub fn display(thread: Option<ThreadId>, embedded: bool) -> Option<String> {
    if !embedded {
        return None;
    }
    let rate = RATES
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(thread?)?;
    Some(format!("{rate:.1} tk/s"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_output_speed_cache_is_bounded_and_invalid_usage_clears_previous_sample() {
        let mut rates = LatestRates::default();
        let first = ThreadId::new();
        rates.record(first, Some(1234), Duration::from_secs(2));
        assert_eq!(rates.get(first), Some(617.0));
        for tokens in [None, Some(-1)] {
            rates.record(first, tokens, Duration::from_secs(1));
            assert_eq!(rates.get(first), None);
        }
        rates.record(first, Some(0), Duration::from_secs(1));
        assert_eq!(rates.get(first), Some(0.0));
        rates.record(first, Some(100), Duration::ZERO);
        assert_eq!(rates.get(first), None);
        rates.record(first, Some(100), Duration::from_secs(1));
        let mut last = first;
        for _ in 0..MAX_THREADS {
            last = ThreadId::new();
            rates.record(last, Some(100), Duration::from_secs(1));
        }
        assert_eq!(rates.0.len(), MAX_THREADS);
        assert_eq!(rates.get(first), None);
        assert_eq!(rates.get(last), Some(100.0));
        rates.record(last, Some(200), Duration::from_secs(1));
        assert_eq!(rates.0.len(), MAX_THREADS);
        assert_eq!(rates.get(last), Some(200.0));
    }

    #[test]
    fn custom_output_speed_never_shares_values_with_daemon_or_remote_clients() {
        let thread = ThreadId::new();
        record(thread, Some(1234), Duration::from_secs(2));
        assert_eq!(display(Some(thread), true).as_deref(), Some("617.0 tk/s"));
        assert_eq!(display(Some(thread), false), None);
        assert_eq!(display(Some(ThreadId::new()), true), None);
        assert_eq!(display(None, true), None);
    }
}
