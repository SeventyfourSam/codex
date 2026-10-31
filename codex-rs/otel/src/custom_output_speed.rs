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
