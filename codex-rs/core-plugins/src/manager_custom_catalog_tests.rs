use super::super::*;
use crate::remote::RemotePluginScope;
use crate::test_support::load_plugins_config;
use crate::test_support::test_plugins_manager;
use chrono::TimeDelta;
use chrono::Utc;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;
use wiremock::matchers::query_param;

#[tokio::test]
async fn catalog_refresh_is_on_demand_and_respects_disk_cache_ttl() {
    // Exercise the fork's real entry point even when the upstream compatibility
    // suite runs with a different policy; avoid mutating process-global env in Rust.
    const CHILD: &str = "CODEX_TEST_CUSTOM_CATALOG_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "manager::custom_catalog::tests::catalog_refresh_is_on_demand_and_respects_disk_cache_ttl",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env_remove("CODEX_CUSTOM_PLUGIN_CATALOG_POLICY")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/backend-api/ps/plugins/list"))
        .and(query_param("scope", "GLOBAL"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "plugins": [], "pagination": {"next_page_token": null},
        })))
        .expect(2)
        .mount(&server)
        .await;
    let directory = tempfile::tempdir().unwrap();
    let manager = Arc::new(test_plugins_manager(directory.path().to_path_buf()));
    let mut config = load_plugins_config(directory.path(), directory.path()).await;
    config.chatgpt_base_url = format!("{}/backend-api", server.uri());
    let auth = CodexAuth::create_dummy_chatgpt_auth_for_testing();
    let request = |mode| {
        manager.maybe_start_remote_catalog_cache_refresh(
            &config,
            Some(auth.clone()),
            BTreeSet::from([RemotePluginScope::Global]),
            mode,
        )
    };
    let assert_idle = || {
        let state = manager.remote_catalog_cache_refresh_state.read().unwrap();
        assert!(!state.in_flight);
        assert!(state.requests.is_empty());
    };

    // Startup must not schedule any work, even with no cached directory.
    request(RemoteCatalogCacheRefreshMode::Force);
    assert_idle();
    assert!(server.received_requests().await.unwrap().is_empty());

    // The first explicit list fetch populates the cache; subsequent opens reuse it.
    request(RemoteCatalogCacheRefreshMode::OnlyIfStale);
    wait_for_refresh(&manager).await;
    assert!(crate::remote::has_fresh_cached_remote_plugin_catalog(
        directory.path(),
        &remote_plugin_service_config(&config),
        Some(&auth),
        RemotePluginScope::Global,
    ));
    request(RemoteCatalogCacheRefreshMode::Force);
    assert_idle();
    request(RemoteCatalogCacheRefreshMode::OnlyIfStale);
    wait_for_refresh(&manager).await;
    assert_eq!(server.received_requests().await.unwrap().len(), 1);

    let cache = std::fs::read_dir(directory.path().join("cache/remote_plugin_catalog"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&cache).unwrap()).unwrap();
    value["fetched_at"] = serde_json::json!(Utc::now() - TimeDelta::hours(4));
    std::fs::write(cache, serde_json::to_vec(&value).unwrap()).unwrap();

    // Expiry alone (or another startup) does not download. Opening the list does.
    request(RemoteCatalogCacheRefreshMode::Force);
    assert_idle();
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
    request(RemoteCatalogCacheRefreshMode::OnlyIfStale);
    wait_for_refresh(&manager).await;
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

async fn wait_for_refresh(manager: &Arc<PluginsManager>) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while manager
            .remote_catalog_cache_refresh_state
            .read()
            .unwrap()
            .in_flight
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}
