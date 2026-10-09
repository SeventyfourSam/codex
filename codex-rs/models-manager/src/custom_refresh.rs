//! Custom catalog policy; upstream refresh strategies and their callers stay intact.

use super::*;

const CUSTOM_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Reasons for a custom catalog refresh, separate from upstream's strategy enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomRefreshPurpose {
    Automatic,
    Startup,
    Manual,
}

impl OpenAiModelsManager {
    /// Opt a production provider into bounded downloads without changing default managers.
    pub fn with_custom_refresh_policy(mut self) -> Self {
        self.refresh_policy.enabled = true;
        if let Some(path) = &self.refresh_policy.path {
            self.cache = Some(Arc::new(FileModelsCache::new(
                path.with_file_name(MODEL_CACHE_FILE),
                CUSTOM_CACHE_TTL,
            )));
        }
        self
    }

    pub(super) fn custom_auth_refresh_strategy(&self) -> RefreshStrategy {
        if self.refresh_policy.enabled {
            RefreshStrategy::Offline
        } else {
            RefreshStrategy::OnlineIfUncached
        }
    }

    pub(super) async fn custom_refreshed_models(
        &self,
        purpose: CustomRefreshPurpose,
        http_client_factory: HttpClientFactory,
    ) -> Vec<ModelPreset> {
        {
            let _refresh_guard = self.catalog_source.refresh_lock().await;
            if !self.api_key_discovery_disabled() && self.should_refresh_models().await {
                if let Err(error) = self
                    .refresh_custom_cached_models(purpose, &http_client_factory)
                    .await
                {
                    error!("failed to refresh custom model catalog: {error}");
                }
            }
        }
        self.list_models(RefreshStrategy::Offline, http_client_factory)
            .await
    }

    #[expect(
        clippy::await_holding_invalid_type,
        reason = "serialize cache checks and downloads so concurrent callers reuse the completed catalog"
    )]
    pub(super) async fn refresh_custom_cached_models(
        &self,
        purpose: CustomRefreshPurpose,
        http_client_factory: &HttpClientFactory,
    ) -> CoreResult<()> {
        let mut state = self.refresh_policy.state.lock().await;
        let cached = self.try_load_cache().await;
        let identity = self.endpoint_client.identity().unwrap_or_default();
        let first_startup = state.initialized.insert(identity.clone());
        if purpose == CustomRefreshPurpose::Automatic && !first_startup {
            return Ok(());
        }
        let interval = if purpose == CustomRefreshPurpose::Manual {
            Duration::from_secs(60 * 60)
        } else {
            CUSTOM_CACHE_TTL
        };
        let fetched_at = if cached {
            Some(self.remote_models.read().await.fetched_at)
        } else {
            None
        };
        if self
            .refresh_policy
            .reserve(&mut state, identity, fetched_at, interval)
            .await?
        {
            self.fetch_and_update_models(http_client_factory).await
        } else {
            // Another terminal may have just populated the shared cache.
            self.try_load_cache().await;
            Ok(())
        }
    }
}
