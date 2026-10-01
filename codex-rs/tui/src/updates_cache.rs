use crate::legacy_core::config::Config;
use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) struct VersionInfo {
    pub(crate) latest_version: String,
    // ISO-8601 timestamp (RFC3339)
    pub(crate) last_checked_at: DateTime<Utc>,
    #[serde(default)]
    pub(crate) dismissed_version: Option<String>,
}

const VERSION_FILENAME: &str = codex_install_context::CUSTOM_VERSION_CACHE;

pub(crate) fn version_filepath(config: &Config) -> PathBuf {
    config.codex_home.join(VERSION_FILENAME).into_path_buf()
}

pub(crate) fn read_version_info(version_file: &Path) -> anyhow::Result<VersionInfo> {
    let contents = std::fs::read_to_string(version_file)?;
    Ok(serde_json::from_str(&contents)?)
}

/// Persist a dismissal for the current latest version so we don't show
/// the update popup again for this version.
pub(crate) async fn dismiss_version(config: &Config, version: &str) -> anyhow::Result<()> {
    let version_file = version_filepath(config);
    let mut info = match read_version_info(&version_file) {
        Ok(info) => info,
        Err(_) => VersionInfo {
            latest_version: version.to_string(),
            last_checked_at: DateTime::<Utc>::UNIX_EPOCH,
            dismissed_version: None,
        },
    };
    info.dismissed_version = Some(version.to_string());
    let json_line = format!("{}\n", serde_json::to_string(&info)?);
    if let Some(parent) = version_file.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(version_file, json_line).await?;
    Ok(())
}

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
#[path = "updates_cache_tests.rs"]
mod tests;
