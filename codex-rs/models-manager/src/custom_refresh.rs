//! Keep the custom cache throttling policy separate from upstream catalog maintenance.

use super::*;

impl OpenAiModelsManager {
    #[expect(
        clippy::await_holding_invalid_type,
        reason = "serialize cache checks and downloads so concurrent callers reuse the completed catalog"
    )]
    pub(super) async fn refresh_custom_cached_models(
        &self,
        refresh_strategy: RefreshStrategy,
        http_client_factory: &HttpClientFactory,
    ) -> CoreResult<()> {
        let mut state = self.refresh_policy.state.lock().await;
        let cached = self.try_load_cache().await;
        let identity = self.endpoint_client.identity().unwrap_or_default();
        let first_startup = state.initialized.insert(identity.clone());
        if refresh_strategy == RefreshStrategy::OnlineIfUncached && !first_startup {
            return Ok(());
        }
        let interval = if refresh_strategy == RefreshStrategy::Manual {
            Duration::from_secs(60 * 60)
        } else {
            DEFAULT_MODEL_CACHE_TTL
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
