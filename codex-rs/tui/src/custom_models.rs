//! Explicit fork catalog refreshes with compatibility for an unmodified remote server.

use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::CustomModelRefreshParams;
use codex_app_server_protocol::CustomModelRefreshPurpose;
use codex_app_server_protocol::ModelListParams;
use codex_app_server_protocol::ModelListResponse;
use codex_app_server_protocol::RequestId;

pub(crate) async fn fetch_models(
    handle: AppServerRequestHandle,
    request_id: RequestId,
    purpose: CustomModelRefreshPurpose,
) -> Result<ModelListResponse, TypedRequestError> {
    let result = handle
        .request_typed(ClientRequest::CustomModelRefresh {
            request_id: request_id.clone(),
            params: CustomModelRefreshParams {
                purpose,
                include_hidden: true,
            },
        })
        .await;
    if result.as_ref().is_err_and(is_unsupported_method) {
        return handle
            .request_typed(ClientRequest::ModelList {
                request_id,
                params: ModelListParams {
                    include_hidden: Some(true),
                    ..Default::default()
                },
            })
            .await;
    }
    result
}

fn is_unsupported_method(error: &TypedRequestError) -> bool {
    matches!(error, TypedRequestError::Server { source, .. }
        if source.code == -32601
        || (source.code == -32600
            && source.message.contains("unknown variant `custom/modelRefresh`")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_app_server_protocol::JSONRPCErrorError;

    #[test]
    fn fallback_only_accepts_an_unsupported_custom_method() {
        for (code, message, expected) in [
            (-32601, "Method not found", true),
            (-32600, "unknown variant `custom/modelRefresh`", true),
            (-32600, "Invalid params", false),
            (-32000, "Unauthenticated", false),
        ] {
            let error = TypedRequestError::Server {
                method: "custom/modelRefresh".to_string(),
                source: JSONRPCErrorError {
                    code,
                    message: message.to_string(),
                    data: None,
                },
            };
            assert_eq!(is_unsupported_method(&error), expected);
        }
    }
}
