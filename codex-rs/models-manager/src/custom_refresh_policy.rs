//! Bound catalog downloads across sessions, including unsuccessful attempts.

use chrono::DateTime;
use chrono::Utc;
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io;
use std::io::Seek;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::Mutex;

#[derive(Debug, Default)]
pub(crate) struct RefreshPolicy {
    // Ordinary constructors and upstream unit tests keep their original behavior.
    pub(crate) enabled: bool,
    pub(crate) path: Option<PathBuf>,
    pub(crate) state: Mutex<RefreshState>,
}

#[derive(Debug, Default)]
pub(crate) struct RefreshState {
    pub(crate) initialized: HashSet<String>,
    attempts: BTreeMap<String, DateTime<Utc>>,
}

impl RefreshPolicy {
    pub(crate) async fn reserve(
        &self,
        state: &mut RefreshState,
        identity: String,
        fetched_at: Option<DateTime<Utc>>,
        interval: Duration,
    ) -> io::Result<bool> {
        let path = self.path.clone();
        let mut attempts = state.attempts.clone();
        let (allowed, attempts) = tokio::task::spawn_blocking(move || -> io::Result<_> {
            let mut file = if let Some(path) = path {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(path)?;
                file.lock()?;
                if file.metadata()?.len() != 0 {
                    attempts = serde_json::from_reader(&file)?;
                }
                Some(file)
            } else {
                None
            };
            let now = Utc::now();
            let last = attempts
                .get(&identity)
                .copied()
                .into_iter()
                .chain(fetched_at)
                .max();
            let allowed = last.is_none_or(|last| {
                now.signed_duration_since(last)
                    .to_std()
                    .is_ok_and(|age| age >= interval)
            });
            if allowed {
                attempts
                    .retain(|_, last| now.signed_duration_since(*last) < chrono::Duration::days(1));
                attempts.insert(identity, now);
                if let Some(file) = file.as_mut() {
                    file.rewind()?;
                    file.set_len(0)?;
                    serde_json::to_writer(&mut *file, &attempts)?;
                    file.flush()?;
                }
            }
            // Dropping the file releases the lock. Reserve before the HTTP call so
            // cancellation and failed downloads cannot cause a retry storm.
            Ok((allowed, attempts))
        })
        .await
        .map_err(io::Error::other)??;
        state.attempts = attempts;
        Ok(allowed)
    }
}
