//! Mention completion needs installed plugins, never the full remote catalog.

use super::super::App;
use super::super::AppServerSession;
use super::super::background_requests;
use super::*;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::PluginInstalledParams;
use codex_app_server_protocol::PluginInstalledResponse;
use codex_app_server_protocol::RequestId;
use codex_utils_absolute_path::AbsolutePathBuf;

fn enabled() -> bool {
    std::env::var("CODEX_CUSTOM_PLUGIN_CATALOG_POLICY").as_deref() != Ok("upstream")
}

fn installed_request(cwd: PathBuf) -> Result<ClientRequest> {
    Ok(ClientRequest::PluginInstalled {
        request_id: RequestId::String(format!("plugin-mentions-{}", Uuid::new_v4())),
        params: PluginInstalledParams {
            cwds: Some(vec![
                AbsolutePathBuf::try_from(cwd).wrap_err("plugin mention cwd must be absolute")?,
            ]),
            install_suggestion_plugin_names: None,
        },
    })
}

pub(super) async fn request_plugin_list(
    request_handle: AppServerRequestHandle,
    cwd: PathBuf,
) -> Result<PluginListResponse> {
    if !enabled() {
        return background_requests::request_plugin_list(request_handle, cwd).await;
    }
    request_installed_plugin_list(request_handle, cwd).await
}

async fn request_installed_plugin_list(
    request_handle: AppServerRequestHandle,
    cwd: PathBuf,
) -> Result<PluginListResponse> {
    let response: PluginInstalledResponse = request_handle
        .request_typed(installed_request(cwd)?)
        .await
        .wrap_err("plugin/installed failed for TUI mentions")?;
    Ok(PluginListResponse {
        marketplaces: response.marketplaces,
        marketplace_load_errors: response.marketplace_load_errors,
        featured_plugin_ids: Vec::new(),
    })
}

pub(in crate::app) fn refresh_visible_catalog(
    app: &mut App,
    app_server: &AppServerSession,
    cwd: PathBuf,
) {
    // An unsolicited import notification must not open/download the catalog.
    // Explicit /plugins and plugin install/uninstall actions keep their normal path.
    if !enabled() || catalog_visible(app) {
        app.fetch_plugins_list(app_server, cwd);
    }
}

fn catalog_visible(app: &App) -> bool {
    app.chat_widget
        .selected_index_for_present_view("plugins-selection")
        .is_some()
}
