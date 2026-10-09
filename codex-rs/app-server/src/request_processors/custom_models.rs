//! Route explicit custom refreshes before using the unchanged model/list handler.

use super::*;
use codex_app_server_protocol::CustomModelRefreshParams;
use codex_app_server_protocol::CustomModelRefreshPurpose;
use codex_app_server_protocol::CustomModelRefreshResponse;
use codex_models_manager::manager::CustomRefreshPurpose;

impl CatalogRequestProcessor {
    pub(crate) async fn custom_model_refresh(
        &self,
        params: CustomModelRefreshParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let purpose = match params.purpose {
            CustomModelRefreshPurpose::Startup => CustomRefreshPurpose::Startup,
            CustomModelRefreshPurpose::Manual => CustomRefreshPurpose::Manual,
        };
        let query = ModelListParams {
            include_hidden: Some(params.include_hidden),
            ..Default::default()
        };
        // Reuse upstream's gateway authentication and provider checks before fetching.
        self.model_list(query.clone()).await?;
        self.model_catalog
            .custom_refresh_models(purpose)
            .await
            .map_err(|error| config_load_error(&error))?;
        self.list_models(query)
            .await
            .map(|catalog| Some(CustomModelRefreshResponse { catalog }.into()))
    }
}
