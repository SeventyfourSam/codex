//! Initialize the custom catalog once; retain upstream's periodic worker for its tests.

use super::*;

pub(super) fn enabled() -> bool {
    true
}

pub(super) fn spawn(model_catalog: &Arc<ModelCatalog>) -> ModelsRefreshWorker {
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
