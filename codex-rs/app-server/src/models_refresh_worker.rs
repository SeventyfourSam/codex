use std::sync::Arc;

use codex_models_manager::manager::RefreshStrategy;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::model_catalog::ModelCatalog;

#[derive(Debug)]
pub(crate) struct ModelsRefreshWorker {
    shutdown: CancellationToken,
    _task: JoinHandle<()>,
}

impl ModelsRefreshWorker {
    pub(crate) fn shutdown(&self) {
        self.shutdown.cancel();
    }
}

impl Drop for ModelsRefreshWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub(crate) fn spawn(model_catalog: &Arc<ModelCatalog>) -> ModelsRefreshWorker {
    let model_catalog = Arc::downgrade(model_catalog);
    let shutdown = CancellationToken::new();
    let worker_shutdown = shutdown.clone();
    let task = tokio::spawn(async move {
        let Some(model_catalog) = model_catalog.upgrade() else {
            return;
        };
        tokio::select! {
            _ = worker_shutdown.cancelled() => {}
            result = model_catalog.list_models(RefreshStrategy::OnlineIfUncached) => {
                if let Err(err) = result {
                    // Parser diagnostics can include provider credentials from the source TOML.
                    tracing::warn!(error_kind = ?err.kind(), "model catalog refresh blocked by provider requirements");
                }
            }
        }
    });
    ModelsRefreshWorker {
        shutdown,
        _task: task,
    }
}

#[cfg(test)]
#[path = "models_refresh_worker_tests.rs"]
mod tests;
