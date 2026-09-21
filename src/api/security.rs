use crate::storage::{AuthAuditEvent, StorageError, StorageReader};
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Serialize;
use std::sync::Arc;
use thiserror::Error;

pub fn router(storage_reader: Arc<StorageReader>) -> Router {
    Router::new()
        .route("/api/security/auth-events", get(auth_events))
        .with_state(storage_reader)
}

async fn auth_events(
    State(storage_reader): State<Arc<StorageReader>>,
) -> Result<Json<AuthAuditCatalog>, SecurityApiError> {
    let events = tokio::task::spawn_blocking(move || storage_reader.auth_audit_events(200)).await??;
    Ok(Json(AuthAuditCatalog { events }))
}

#[derive(Serialize)]
struct AuthAuditCatalog {
    events: Vec<AuthAuditEvent>,
}

#[derive(Debug, Error)]
enum SecurityApiError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("security API task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}

impl IntoResponse for SecurityApiError {
    fn into_response(self) -> Response {
        (StatusCode::BAD_GATEWAY, self.to_string()).into_response()
    }
}
