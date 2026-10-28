use super::*;
use pretty_assertions::assert_eq;

#[derive(Debug)]
struct OriginatorEndpoint {
    originator: &'static str,
    calls: Arc<Mutex<std::collections::BTreeMap<&'static str, usize>>>,
}

impl ModelsEndpointClient for OriginatorEndpoint {
    fn for_originator(
        &self,
        originator: crate::ModelCatalogOriginator,
    ) -> Option<Arc<dyn ModelsEndpointClient>> {
        Some(Arc::new(Self {
            originator: originator.as_str(),
            calls: Arc::clone(&self.calls),
        }))
    }

    fn identity(&self) -> Option<String> {
        Some(self.originator.to_owned())
    }

    fn has_command_auth(&self) -> bool {
        false
    }

    fn uses_codex_backend(&self) -> ModelsEndpointFuture<'_, bool> {
        Box::pin(std::future::ready(true))
    }

    fn list_models<'a>(
        &'a self,
        _client_version: &'a str,
        _http_client_factory: HttpClientFactory,
    ) -> ModelsEndpointFuture<'a, CoreResult<ModelsEndpointResponse>> {
        Box::pin(async move {
            *self
                .calls
                .lock()
                .unwrap()
                .entry(self.originator)
                .or_default() += 1;
            Ok(ModelsEndpointResponse {
                models: vec![remote_model(self.originator, self.originator, 0)],
                etag: Some("initial".into()),
                identity: self.originator.to_owned(),
            })
        })
    }
}

#[tokio::test]
async fn originator_catalogs_inherit_throttling_without_losing_cache_isolation() {
    let home = tempdir().unwrap();
    let calls = Arc::new(Mutex::new(std::collections::BTreeMap::new()));
    let endpoint = Arc::new(OriginatorEndpoint {
        originator: "default",
        calls: Arc::clone(&calls),
    });
    let scopes = ["default", "codex_work_cca", "chatgpt_cca"];
    // A restarted manager must reuse catalogs older than upstream's five-minute TTL.
    for _ in 0..2 {
        let parent: SharedModelsManager = Arc::new(
            openai_manager_for_tests(home.path().to_path_buf(), endpoint.clone())
                .with_custom_refresh_policy(),
        );
        for scope in scopes {
            let manager = parent
                .for_originator(scope)
                .unwrap_or_else(|| parent.clone());
            let catalog = manager
                .raw_model_catalog(
                    RefreshStrategy::OnlineIfUncached,
                    DEFAULT_HTTP_CLIENT_FACTORY,
                )
                .await;
            assert!(catalog.models.iter().any(|model| model.slug == scope));
            manager
                .refresh_if_new_etag("changed".into(), DEFAULT_HTTP_CLIENT_FACTORY)
                .await;
            assert_eq!(calls.lock().unwrap().get(scope), Some(&1));

            let file = if scope == "default" {
                home.path().join(MODEL_CACHE_FILE)
            } else {
                home.path().join(format!("models_cache.{scope}.json"))
            };
            let mut entry: ModelsCacheEntry =
                serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
            assert_eq!(entry.identity.as_deref(), Some(scope));
            entry.fetched_at = Utc::now() - chrono::Duration::hours(2);
            std::fs::write(file, serde_json::to_vec(&entry).unwrap()).unwrap();
        }
    }
    // Losing a scoped cache must not bypass the parent's persisted download reservation.
    std::fs::remove_file(home.path().join("models_cache.codex_work_cca.json")).unwrap();
    let restarted =
        openai_manager_for_tests(home.path().to_path_buf(), endpoint).with_custom_refresh_policy();
    restarted
        .for_originator("codex_work_cca")
        .unwrap()
        .list_models(
            RefreshStrategy::OnlineIfUncached,
            DEFAULT_HTTP_CLIENT_FACTORY,
        )
        .await;
    assert_eq!(calls.lock().unwrap().get("codex_work_cca"), Some(&1));
}

#[tokio::test]
async fn inference_etags_do_not_download_or_extend_the_catalog_cache() {
    let home = tempdir().unwrap();
    let endpoint = TestModelsEndpoint::new(vec![vec![remote_model(
        "remote", "Remote", /*priority*/ 0,
    )]]);
    let manager = openai_manager_for_tests(home.path().to_path_buf(), endpoint.clone())
        .with_custom_refresh_policy();
    manager
        .custom_refresh_models(CustomRefreshPurpose::Startup, DEFAULT_HTTP_CLIENT_FACTORY)
        .await;
    let path = home.path().join(MODEL_CACHE_FILE);
    let cached = std::fs::read(&path).unwrap();
    for etag in ["test-etag", "changed-etag"] {
        ModelsManager::refresh_if_new_etag(&manager, etag.into(), DEFAULT_HTTP_CLIENT_FACTORY)
            .await;
    }
    assert_eq!(endpoint.fetch_count(), 1);
    assert_eq!(std::fs::read(path).unwrap(), cached);
}

#[tokio::test]
async fn startup_and_manual_refresh_use_distinct_cache_windows() {
    for (age_hours, expected_startup, expected_manual) in
        [(0, 0, 0), (2, 0, 1), (23, 0, 1), (25, 1, 1)]
    {
        let home = tempdir().unwrap();
        let initial = remote_model("cached", "Cached", /*priority*/ 0);
        let cache = FileModelsCache::new(
            home.path().join(MODEL_CACHE_FILE),
            Duration::from_secs(24 * 60 * 60),
        );
        cache
            .store(&ModelsCacheEntry {
                fetched_at: Utc::now() - chrono::Duration::hours(age_hours),
                etag: Some("cached".into()),
                client_version: Some(crate::client_version_to_whole()),
                identity: Some("test-provider".into()),
                models: vec![initial],
            })
            .await
            .unwrap();
        let endpoint = TestModelsEndpoint::new(vec![vec![remote_model(
            "updated", "Updated", /*priority*/ 0,
        )]]);
        let manager = openai_manager_for_tests(home.path().to_path_buf(), endpoint.clone())
            .with_custom_refresh_policy();
        manager
            .custom_refresh_models(CustomRefreshPurpose::Startup, DEFAULT_HTTP_CLIENT_FACTORY)
            .await;
        assert_eq!(endpoint.fetch_count(), expected_startup);
        manager
            .custom_refresh_models(CustomRefreshPurpose::Manual, DEFAULT_HTTP_CLIENT_FACTORY)
            .await;
        assert_eq!(endpoint.fetch_count(), expected_manual);
        // A second process obeys the same hourly limit, including a cold in-memory catalog.
        let restarted = openai_manager_for_tests(home.path().to_path_buf(), endpoint.clone())
            .with_custom_refresh_policy();
        restarted
            .custom_refresh_models(CustomRefreshPurpose::Manual, DEFAULT_HTTP_CLIENT_FACTORY)
            .await;
        assert_eq!(endpoint.fetch_count(), expected_manual);
    }
}

#[tokio::test]
async fn a_new_frontend_startup_can_refresh_an_existing_daemon_catalog() {
    let home = tempdir().unwrap();
    let model = remote_model("remote", "Remote", /*priority*/ 0);
    let endpoint = TestModelsEndpoint::new(vec![vec![model.clone()], vec![model]]);
    let manager = openai_manager_for_tests(home.path().to_path_buf(), endpoint.clone())
        .with_custom_refresh_policy();
    manager
        .custom_refresh_models(CustomRefreshPurpose::Startup, DEFAULT_HTTP_CLIENT_FACTORY)
        .await;
    mutate_file_cache_for_test(home.path(), |entry| {
        entry.fetched_at = Utc::now() - chrono::Duration::hours(25);
    })
    .await;
    std::fs::write(
        home.path().join("models_refresh.json"),
        serde_json::to_vec(&json!({
            "test-provider": Utc::now() - chrono::Duration::hours(25),
        }))
        .unwrap(),
    )
    .unwrap();
    manager
        .list_models(
            RefreshStrategy::OnlineIfUncached,
            DEFAULT_HTTP_CLIENT_FACTORY,
        )
        .await;
    assert_eq!(endpoint.fetch_count(), 1);
    manager
        .custom_refresh_models(CustomRefreshPurpose::Startup, DEFAULT_HTTP_CLIENT_FACTORY)
        .await;
    assert_eq!(endpoint.fetch_count(), 2);
}

#[tokio::test]
async fn concurrent_startup_managers_share_a_download_reservation() {
    let home = tempdir().unwrap();
    let endpoint = TestModelsEndpoint::new(vec![vec![remote_model(
        "remote", "Remote", /*priority*/ 0,
    )]]);
    let first = openai_manager_for_tests(home.path().to_path_buf(), endpoint.clone())
        .with_custom_refresh_policy();
    let second = openai_manager_for_tests(home.path().to_path_buf(), endpoint.clone())
        .with_custom_refresh_policy();
    tokio::join!(
        first.list_models(
            RefreshStrategy::OnlineIfUncached,
            DEFAULT_HTTP_CLIENT_FACTORY
        ),
        second.list_models(
            RefreshStrategy::OnlineIfUncached,
            DEFAULT_HTTP_CLIENT_FACTORY
        ),
    );
    assert_eq!(endpoint.fetch_count(), 1);
}

#[tokio::test]
async fn persisted_reservation_throttles_attempts_even_without_a_successful_cache_write() {
    use crate::custom_refresh_policy::RefreshPolicy;
    let home = tempdir().unwrap();
    let path = home.path().join("models_refresh.json");
    let policy = RefreshPolicy {
        path: Some(path.clone()),
        ..Default::default()
    };
    let mut state = Default::default();
    assert!(
        policy
            .reserve(
                &mut state,
                "account-a".into(),
                /*fetched_at*/ None,
                Duration::from_secs(24 * 60 * 60)
            )
            .await
            .unwrap()
    );
    let restarted = RefreshPolicy {
        path: Some(path),
        ..Default::default()
    };
    let mut state = Default::default();
    assert!(
        !restarted
            .reserve(
                &mut state,
                "account-a".into(),
                /*fetched_at*/ None,
                Duration::from_secs(/*secs*/ 3600)
            )
            .await
            .unwrap()
    );
    assert!(
        restarted
            .reserve(
                &mut state,
                "account-b".into(),
                /*fetched_at*/ None,
                Duration::from_secs(24 * 60 * 60)
            )
            .await
            .unwrap()
    );
}
