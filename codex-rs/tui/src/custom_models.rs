//! Explicit fork catalog refreshes with compatibility for an unmodified remote server.

use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::CustomModelRefreshParams;
use codex_app_server_protocol::CustomModelRefreshPurpose;
use codex_app_server_protocol::ModelListParams;
use codex_app_server_protocol::ModelListResponse;
use codex_app_server_protocol::RequestId;
use serde::de::DeserializeOwned;
use std::future::Future;

/// Keep the upstream request and its parameters at the startup call site.
pub(crate) trait StartupModelsRequestExt {
    fn custom_startup_request<T: DeserializeOwned>(
        &self,
        request: ClientRequest,
    ) -> impl Future<Output = Result<T, TypedRequestError>>;
}

impl StartupModelsRequestExt for AppServerRequestHandle {
    async fn custom_startup_request<T: DeserializeOwned>(
        &self,
        request: ClientRequest,
    ) -> Result<T, TypedRequestError> {
        request_models(self, request, CustomModelRefreshPurpose::Startup).await
    }
}

pub(crate) async fn fetch_models(
    handle: AppServerRequestHandle,
    request_id: RequestId,
    purpose: CustomModelRefreshPurpose,
) -> Result<ModelListResponse, TypedRequestError> {
    request_models(
        &handle,
        ClientRequest::ModelList {
            request_id,
            params: ModelListParams {
                include_hidden: Some(true),
                ..Default::default()
            },
        },
        purpose,
    )
    .await
}

async fn request_models<T: DeserializeOwned>(
    handle: &AppServerRequestHandle,
    request: ClientRequest,
    purpose: CustomModelRefreshPurpose,
) -> Result<T, TypedRequestError> {
    // Hosted sessions expose the upstream catalog; custom refreshes belong to our server.
    if !matches!(handle, AppServerRequestHandle::Cloud(_))
        && let ClientRequest::ModelList { request_id, params } = &request
        // Preserve pagination and any future non-default upstream parameters.
        && params == &(ModelListParams {
            include_hidden: params.include_hidden,
            ..Default::default()
        })
    {
        let result = handle
            .request_typed::<T>(ClientRequest::CustomModelRefresh {
                request_id: request_id.clone(),
                params: CustomModelRefreshParams {
                    purpose,
                    include_hidden: params.include_hidden.unwrap_or(false),
                },
            })
            .await;
        if !result.as_ref().is_err_and(is_unsupported_method) {
            return result;
        }
    }
    handle.request_typed(request).await
}

fn is_unsupported_method(error: &TypedRequestError) -> bool {
    matches!(error, TypedRequestError::Server { source, .. }
        if source.code == -32601
        || (source.code == -32600
            && source.message.contains("unknown variant `custom/modelRefresh`")))
}
