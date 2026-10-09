//! Daily cross-process reservation for custom release checks.

use chrono::{DateTime, Utc};
use std::path::Path;

/// Reserve before querying so concurrent launches and failed requests share a daily budget.
#[cfg(any(not(debug_assertions), test))]
pub(crate) fn reserve_update_check(
    version_file: &Path,
    now: DateTime<Utc>,
) -> anyhow::Result<bool> {
    use std::io::Seek;
    if let Some(parent) = version_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(version_file.with_extension("check"))?;
    file.lock()?;
    if file.metadata()?.len() != 0 {
        let last: DateTime<Utc> = serde_json::from_reader(&file)?;
        if now.signed_duration_since(last) < chrono::Duration::hours(24) {
            return Ok(false);
        }
    }
    file.rewind()?;
    file.set_len(0)?;
    serde_json::to_writer(&mut file, &now)?;
    file.sync_all()?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launches_share_a_daily_reservation_even_without_a_successful_release_check() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("custom-version.json");
        let now = Utc::now();
        assert!(reserve_update_check(&path, now).unwrap());
        assert!(!reserve_update_check(&path, now + chrono::Duration::hours(23)).unwrap());
        assert!(reserve_update_check(&path, now + chrono::Duration::hours(24)).unwrap());
        assert!(!path.exists());
    }
}
