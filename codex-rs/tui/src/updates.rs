#![cfg(any(not(debug_assertions), test))]

use crate::legacy_core::config::Config;
use crate::update_action;
use crate::update_versions::is_newer;
use crate::update_versions::is_source_build_version;
use crate::updates_cache::VersionInfo;
use crate::updates_cache::read_version_info;
use crate::updates_cache::version_filepath;
use chrono::Duration;
use chrono::Utc;
use codex_http_client::ClientRouteClass;
use codex_http_client::HttpClientFactory;
use codex_http_client::RouteAwareClientPool;
use codex_install_context::CustomRelease;
use codex_login::default_client::default_headers;
use std::path::Path;

use crate::version::CODEX_CLI_VERSION;

pub(crate) use crate::updates_cache::dismiss_version;

pub fn get_upgrade_version(config: &Config) -> Option<String> {
    if !config.check_for_update_on_startup || is_source_build_version(CODEX_CLI_VERSION) {
        return None;
    }

    update_action::get_update_action()?;
    let version_file = version_filepath(config);
    let info = read_version_info(&version_file).ok();

    if match &info {
        None => true,
        Some(info) => info.last_checked_at < Utc::now() - Duration::hours(24),
    } {
        let http_client_factory = config.http_client_factory();
        // Refresh the cached latest version in the background so TUI startup
        // isn’t blocked by a network call. The UI reads the previously cached
        // value (if any) for this run; the next run shows the banner if needed.
        tokio::spawn(async move {
            check_for_update(
                &version_file,
                http_client_factory,
                codex_install_context::CUSTOM_RELEASE_API,
            )
            .await
            .inspect_err(|e| tracing::error!("Failed to update version: {e}"))
        });
    }

    info.and_then(|info| {
        if is_newer(&info.latest_version, codex_utils_cli::CUSTOM_VERSION).unwrap_or(false) {
            Some(info.latest_version)
        } else {
            None
        }
    })
}

async fn check_for_update(
    version_file: &Path,
    http_client_factory: HttpClientFactory,
    url: &str,
) -> anyhow::Result<()> {
    let path = version_file.to_owned();
    if !tokio::task::spawn_blocking(move || {
        crate::updates_cache::reserve_update_check(&path, Utc::now())
    })
    .await??
    {
        return Ok(());
    }
    let client_pool = RouteAwareClientPool::with_chatgpt_cloudflare_cookies(
        http_client_factory,
        ClientRouteClass::Other,
    )
    .with_legacy_custom_ca_fallback();
    let latest_version = fetch_latest_github_release_version(&client_pool, url).await?;

    // Preserve any previously dismissed version if present.
    let prev_info = read_version_info(version_file).ok();
    let info = VersionInfo {
        latest_version,
        last_checked_at: Utc::now(),
        dismissed_version: prev_info.and_then(|p| p.dismissed_version),
    };

    let json_line = format!("{}\n", serde_json::to_string(&info)?);
    if let Some(parent) = version_file.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(version_file, json_line).await?;
    Ok(())
}

async fn fetch_latest_github_release_version(
    client_pool: &RouteAwareClientPool,
    url: &str,
) -> anyhow::Result<String> {
    let mut response = client_pool
        .get(url)
        .timeout(std::time::Duration::from_secs(15))
        .headers(default_headers())
        .send()
        .await?
        .error_for_status()?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        anyhow::ensure!(
            chunk.len() <= (1024_usize * 1024).saturating_sub(bytes.len()),
            "Release metadata exceeds size limit"
        );
        bytes.extend_from_slice(&chunk);
    }
    let release: CustomRelease = serde_json::from_slice(&bytes)?;
    release
        .version()
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("Incomplete or invalid custom release"))
}

/// Returns the latest version to show in a popup, if it should be shown.
/// This respects the user's dismissal choice for the current latest version.
pub fn get_upgrade_version_for_popup(config: &Config) -> Option<String> {
    if !config.check_for_update_on_startup || is_source_build_version(CODEX_CLI_VERSION) {
        return None;
    }

    let version_file = version_filepath(config);
    let latest = get_upgrade_version(config)?;
    // If the user dismissed this exact version previously, do not show the popup.
    if let Ok(info) = read_version_info(&version_file)
        && info.dismissed_version.as_deref() == Some(latest.as_str())
    {
        return None;
    }
    Some(latest)
}

#[cfg(test)]
#[path = "updates_tests.rs"]
mod tests;
