//! Do not refresh the browsable catalog for unsolicited import notifications.

use super::App;
use super::AppServerSession;
use std::path::PathBuf;

fn enabled() -> bool {
    std::env::var("CODEX_CUSTOM_PLUGIN_CATALOG_POLICY").as_deref() != Ok("upstream")
}

pub(in crate::app) fn refresh(app: &mut App, app_server: &AppServerSession, cwd: PathBuf) {
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
