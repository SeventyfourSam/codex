use super::*;
use crate::legacy_core::config::ConfigBuilder;
use codex_http_client::OutboundProxyPolicy;
use pretty_assertions::assert_eq;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

#[tokio::test]
async fn failed_checks_are_reserved_before_http_and_do_not_erase_cache() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/latest"))
        .respond_with(ResponseTemplate::new(/*s*/ 503))
        .expect(/*r*/ 1)
        .mount(&server)
        .await;
    let home = tempfile::tempdir().unwrap();
    let cache = home.path().join("custom-version.json");
    let old = r#"{"latest_version":"0.159.0-custom.1","last_checked_at":"2020-01-01T00:00:00Z","dismissed_version":null}"#;
    std::fs::write(&cache, old).unwrap();
    let url = format!("{}/latest", server.uri());
    let factory = HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault);
    assert!(
        check_for_update(&cache, factory.clone(), &url)
            .await
            .is_err()
    );
    check_for_update(&cache, factory, &url).await.unwrap();
    assert_eq!(std::fs::read_to_string(&cache).unwrap(), old);
}

#[tokio::test]
async fn refreshes_complete_custom_release_and_respects_popup_dismissal() {
    let server = MockServer::start().await;
    let version = "999.0.0-custom.10";
    Mock::given(method("GET"))
        .and(path("/latest"))
        .respond_with(
            ResponseTemplate::new(/*s*/ 200).set_body_json(serde_json::json!({
                "tag_name": format!("v{version}"), "draft": false, "prerelease": false,
                "assets": [
                    {"name": format!("codex-{version}-aarch64-apple-darwin.tar.gz")},
                    {"name": format!("codex-{version}-x86_64-pc-windows-msvc.zip")},
                    {"name": "SHA256SUMS"}
                ]
            })),
        )
        .expect(/*r*/ 1)
        .mount(&server)
        .await;
    let home = tempfile::tempdir().unwrap();
    let mut config = ConfigBuilder::default()
        .codex_home(home.path().to_owned())
        .build()
        .await
        .unwrap();
    config.check_for_update_on_startup = true;
    let cache = version_filepath(&config);
    check_for_update(
        &cache,
        HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
        &format!("{}/latest", server.uri()),
    )
    .await
    .unwrap();
    assert_eq!(read_version_info(&cache).unwrap().latest_version, version);
    // Unsupported platforms must not offer an installer for another architecture.
    if crate::custom_updates::get_update_action().is_some() {
        assert_eq!(
            get_upgrade_version_for_popup(&config),
            Some(version.to_owned())
        );
        dismiss_version(&config, version).await.unwrap();
        assert_eq!(get_upgrade_version_for_popup(&config), None);
    }
    config.check_for_update_on_startup = false;
    assert_eq!(get_upgrade_version(&config), None);
}
