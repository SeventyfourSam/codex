//! Custom release diagnostics keep upstream update adapters and tests intact.

use super::*;

pub(super) async fn updates_check(config: &Config) -> DoctorCheck {
    let mut details = vec![
        format!(
            "check for update on startup: {}",
            config.check_for_update_on_startup
        ),
        "update action: codex update (GitHub custom release)".to_string(),
    ];
    let version_file = config
        .codex_home
        .join(codex_install_context::CUSTOM_VERSION_CACHE);
    push_cached_version_details(&mut details, &version_file);

    let mut status = CheckStatus::Ok;
    let summary = "update configuration is locally consistent".to_string();

    let client = RouteAwareClientPool::new_without_request_logging(
        config.http_client_factory(),
        ClientRouteClass::Other,
    );

    match fetch_latest_custom_version(&client).await {
        Ok(latest_version) => {
            details.push(format!("latest version: {latest_version}"));
            if crate::custom_cli::is_newer(&latest_version, codex_utils_cli::CUSTOM_VERSION)
                == Some(true)
            {
                details.push("latest version status: newer version is available".to_string());
            } else {
                details.push("latest version status: current version is not older".to_string());
            }
        }
        Err(err) => {
            status = status.max(CheckStatus::Warning);
            details.push(format!("latest version probe: {err}"));
        }
    }

    DoctorCheck::new("updates.status", "updates", status, summary).details(details)
}

async fn fetch_latest_custom_version(client: &RouteAwareClientPool) -> Result<String, String> {
    let info = http_get_json::<codex_install_context::CustomRelease>(
        client,
        codex_install_context::CUSTOM_RELEASE_API,
    )
    .await?;
    info.version()
        .map(str::to_owned)
        .ok_or_else(|| "Incomplete or invalid custom release".to_string())
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) fn skip_desktop_update(checks: &mut [DoctorCheck], application: &InstalledApp) -> bool {
    if let Some(update) = checks.iter_mut().find(|check| check.id == "updates.status") {
        update.details.push(format!(
            "desktop update check skipped: {} is outside the custom CLI distribution",
            application.identity
        ));
    }
    true
}
