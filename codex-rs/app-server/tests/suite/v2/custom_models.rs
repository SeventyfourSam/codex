//! Custom requests reuse the official catalog response and authorization checks.

use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::CustomModelRefreshParams;
use codex_app_server_protocol::CustomModelRefreshPurpose;
use codex_app_server_protocol::ModelListParams;
use codex_app_server_protocol::ModelListResponse;

#[tokio::test]
async fn custom_refresh_and_model_list_share_the_catalog_response_shape() -> anyhow::Result<()> {
    let mut server = TestAppServer::builder().build().await?;
    server.initialize().await?;
    let initial: ModelListResponse = server
        .request(|request_id| ClientRequest::ModelList {
            request_id,
            params: ModelListParams {
                include_hidden: Some(true),
                ..Default::default()
            },
        })
        .await?;
    for purpose in [
        CustomModelRefreshPurpose::Startup,
        CustomModelRefreshPurpose::Manual,
    ] {
        let refreshed: ModelListResponse = server
            .request(|request_id| ClientRequest::CustomModelRefresh {
                request_id,
                params: CustomModelRefreshParams {
                    purpose,
                    include_hidden: true,
                },
            })
            .await?;
        assert_eq!(initial, refreshed);
    }
    Ok(())
}
