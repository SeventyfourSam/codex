use codex_config::CloudConfigBundle;
use codex_config::test_support::CloudConfigBundleFixture;
use codex_feedback::CodexFeedback;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tracing_subscriber::layer::SubscriberExt;

use codex_config::CloudConfigBundleLoader;
use codex_config::LoaderOverrides;
use codex_core::config::ConfigBuilder;
use codex_core::config::ConfigOverrides;
use codex_http_client::HttpClientFactory;
use codex_models_manager::manager::ModelsEndpointClient;
use codex_models_manager::manager::ModelsEndpointFuture;
use codex_models_manager::manager::ModelsEndpointResponse;
use codex_models_manager::manager::OpenAiModelsManager;
use codex_models_manager::manager::SharedModelsManager;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CoreResult;
use pretty_assertions::assert_eq;
use tempfile::tempdir;
use tokio::sync::Notify;

use super::*;

#[derive(Debug)]
struct TestModelsEndpoint {
    fetch_count: AtomicUsize,
    fetched: Notify,
    release_second_fetch: Notify,
}

impl TestModelsEndpoint {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            fetch_count: AtomicUsize::new(0),
            fetched: Notify::new(),
            release_second_fetch: Notify::new(),
        })
    }

    async fn wait_for_fetch_count(&self, expected: usize) {
        tokio::time::timeout(Duration::from_secs(1), async {
            while self.fetch_count.load(Ordering::SeqCst) < expected {
                self.fetched.notified().await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("expected {expected} model fetches"));
    }
}

impl ModelsEndpointClient for TestModelsEndpoint {
    fn identity(&self) -> Option<String> {
        Some("test-provider".to_string())
    }

    fn has_command_auth(&self) -> bool {
        true
    }

    fn uses_codex_backend(&self) -> ModelsEndpointFuture<'_, bool> {
        Box::pin(async { false })
    }

    fn list_models<'a>(
        &'a self,
        _client_version: &'a str,
        _http_client_factory: HttpClientFactory,
    ) -> ModelsEndpointFuture<'a, CoreResult<ModelsEndpointResponse>> {
        Box::pin(async move {
            let fetch_index = self.fetch_count.fetch_add(1, Ordering::SeqCst);
            self.fetched.notify_one();
            if fetch_index == 0 {
                return Err(CodexErr::Io(std::io::Error::other("test failure")));
            }
            if fetch_index == 1 {
                self.release_second_fetch.notified().await;
            }
            Ok(ModelsEndpointResponse {
                models: Vec::new(),
                etag: None,
                identity: self.identity().expect("test endpoint identity"),
            })
        })
    }
}

#[tokio::test]
async fn startup_refresh_checks_provider_requirements() {
    let codex_home = tempdir().expect("temp dir");
    let endpoint = TestModelsEndpoint::new();
    let requirements_path = codex_home.path().join("requirements.toml");
    std::fs::write(&requirements_path, "model_provider = 'ollama'").unwrap();
    let config_manager = crate::config_manager::ConfigManager::new_for_tests(
        codex_home.path().to_path_buf(),
        Vec::new(),
        LoaderOverrides::with_managed_config_path_for_tests(
            codex_home.path().join("managed_config.toml"),
        ),
        CloudConfigBundleLoader::default(),
    );
    // --oss selects the provider through typed overrides, outside ConfigManager's raw -c flags.
    let config = Arc::new(
        ConfigBuilder::default()
            .codex_home(codex_home.path().to_path_buf())
            .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
            .harness_overrides(ConfigOverrides {
                model_provider: Some("ollama".to_string()),
                ..Default::default()
            })
            .build()
            .await
            .unwrap(),
    );
    let models_manager: SharedModelsManager = Arc::new(OpenAiModelsManager::new(
        codex_home.path().to_path_buf(),
        endpoint.clone(),
        /*auth_manager*/ None,
    ));
    let catalog = Arc::new(ModelCatalog::new(config_manager, config, models_manager));
    let worker = spawn(&catalog);

    endpoint.wait_for_fetch_count(/*expected*/ 1).await;
    drop(worker);

    assert_eq!(endpoint.fetch_count.load(Ordering::SeqCst), 1);

    // Reloading config would select the newly required provider. The catalog must
    // keep validating the typed startup selection, even when only reading its cache.
    std::fs::write(&requirements_path, "model_provider = 'openai'").unwrap();
    let error = catalog
        .list_models(RefreshStrategy::Offline)
        .await
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(
        error
            .get_ref()
            .unwrap()
            .is::<crate::config_manager::ModelProviderRequirementsChanged>()
    );
    assert_eq!(endpoint.fetch_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn startup_refresh_validates_requirements_before_any_download() -> anyhow::Result<()> {
    let home = tempdir()?;
    let config = Arc::new(
        crate::config_manager::ConfigManager::without_managed_config_for_tests(
            home.path().to_path_buf(),
        )
        .load_non_project_config()
        .await?,
    );
    let (checks_tx, mut checks_rx) = mpsc::unbounded_channel();
    let loader = CloudConfigBundleLoader::from_getter(move || {
        let (reply_tx, reply_rx) = oneshot::channel::<Option<CloudConfigBundle>>();
        checks_tx
            .send(reply_tx)
            .expect("requirements check receiver");
        async move { Ok(reply_rx.await.expect("requirements check response")) }
    });
    let config_manager = crate::config_manager::ConfigManager::new_for_tests(
        home.path().to_path_buf(),
        Vec::new(),
        LoaderOverrides::without_managed_config_for_tests(),
        loader,
    );
    let endpoint = TestModelsEndpoint::new();
    let models_manager: SharedModelsManager = Arc::new(OpenAiModelsManager::new(
        home.path().to_path_buf(),
        endpoint.clone(),
        /*auth_manager*/ None,
    ));
    let catalog = Arc::new(ModelCatalog::new(config_manager, config, models_manager));
    let feedback = CodexFeedback::new();
    // This test uses Tokio's current-thread runtime, so spawned refreshes share the subscriber.
    let _subscriber_guard = tracing::subscriber::set_default(
        tracing_subscriber::registry().with(feedback.logger_layer()),
    );
    for requirements in [
        "model_provider = 'other'",
        "model_provider = []",
        "[model_providers.gateway]\nexperimental_bearer_token = 'private-provider-token-marker' trailing",
    ] {
        let mut worker = spawn(&catalog);
        checks_rx
            .recv()
            .await
            .unwrap()
            .send(Some(
                CloudConfigBundleFixture::enterprise_requirement(requirements).into_bundle(),
            ))
            .unwrap();
        (&mut worker._task).await?;
        assert_eq!(endpoint.fetch_count.load(Ordering::SeqCst), 0);
    }
    let mut worker = spawn(&catalog);
    checks_rx.recv().await.unwrap().send(/*t*/ None).unwrap();
    (&mut worker._task).await?;
    assert_eq!(endpoint.fetch_count.load(Ordering::SeqCst), 1);
    let logs = String::from_utf8(
        feedback
            .snapshot(/*session_id*/ None)
            .log_attachment(/*logs_override*/ None)
            .buffer,
    )?;
    assert!(logs.contains("model catalog refresh blocked by provider requirements"));
    assert!(logs.contains("error_kind=PermissionDenied"));
    assert!(logs.contains("error_kind=InvalidData"));
    assert!(!logs.contains("private-provider-token-marker"));
    Ok(())
}
