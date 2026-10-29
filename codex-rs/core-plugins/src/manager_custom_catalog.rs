//! Keep full catalog downloads on demand; installed-plugin synchronization is separate.

use super::RemoteCatalogCacheRefreshMode;

pub(super) fn allow_refresh(mode: RemoteCatalogCacheRefreshMode) -> bool {
    // Only startup queues Force. Explicit plugin/list requests queue OnlyIfStale,
    // while an explicit force_refetch downloads directly through the listing API.
    mode == RemoteCatalogCacheRefreshMode::OnlyIfStale
        || std::env::var("CODEX_CUSTOM_PLUGIN_CATALOG_POLICY").as_deref() == Ok("upstream")
}

#[cfg(test)]
#[path = "manager_custom_catalog_tests.rs"]
mod tests;
