//! Custom catalog requests do not add fields to upstream model/list parameters.

use crate::JsonSchema;
use crate::TS;
use serde::Deserialize;
use serde::Serialize;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum CustomModelRefreshPurpose {
    Startup,
    Manual,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct CustomModelRefreshParams {
    pub purpose: CustomModelRefreshPurpose,
    pub include_hidden: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct CustomModelRefreshResponse {
    #[serde(flatten)]
    #[ts(flatten)]
    pub catalog: super::ModelListResponse,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ClientRequest;
    use crate::ModelListParams;
    use crate::ModelListResponse;
    use crate::RequestId;
    use serde_json::json;

    #[test]
    fn custom_refresh_keeps_the_upstream_catalog_response_shape() {
        let catalog = ModelListResponse {
            data: vec![],
            next_cursor: None,
        };
        let value = serde_json::to_value(CustomModelRefreshResponse {
            catalog: catalog.clone(),
        })
        .unwrap();
        assert_eq!(
            serde_json::from_value::<ModelListResponse>(value).unwrap(),
            catalog
        );
    }

    #[test]
    fn custom_method_does_not_change_model_list_wire_parameters() {
        let request = ClientRequest::CustomModelRefresh {
            request_id: RequestId::Integer(1),
            params: CustomModelRefreshParams {
                purpose: CustomModelRefreshPurpose::Manual,
                include_hidden: true,
            },
        };
        let value = serde_json::to_value(request).unwrap();
        assert_eq!(value["method"], "custom/modelRefresh");
        assert_eq!(
            value["params"],
            json!({"purpose": "manual", "includeHidden": true})
        );
        let stock = serde_json::to_value(ModelListParams::default()).unwrap();
        assert!(stock.get("refresh").is_none());
    }
}
