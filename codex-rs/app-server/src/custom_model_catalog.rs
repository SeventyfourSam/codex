//! Apply current provider requirements to explicit custom refresh requests.

use super::*;
use codex_models_manager::manager::CustomRefreshPurpose;

impl ModelCatalog {
    pub(crate) async fn custom_refresh_models(
        &self,
        purpose: CustomRefreshPurpose,
    ) -> std::io::Result<Vec<ModelPreset>> {
        let (config, models_manager) = self.current().await?;
        self.config_manager
            .check_thread_model_provider(&config)
            .await?;
        Ok(models_manager
            .custom_refresh_models(purpose, config.http_client_factory())
            .await)
    }
}
