use super::*;
use pretty_assertions::assert_eq;

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
