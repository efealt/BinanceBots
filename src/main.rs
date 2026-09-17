use axum::{Json, Router, routing::get};
use serde::Serialize;
use std::{net::SocketAddr, path::PathBuf};
use tower_http::services::ServeDir;

#[derive(Serialize)]
struct SystemStatus {
    exchange: StatusItem,
    execution: StatusItem,
    storage: StatusItem,
}

#[derive(Serialize)]
struct StatusItem {
    title: &'static str,
    detail: &'static str,
    state: &'static str,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

async fn status() -> Json<SystemStatus> {
    Json(SystemStatus {
        exchange: StatusItem {
            title: "Binance API",
            detail: "Not connected",
            state: "pending",
        },
        execution: StatusItem {
            title: "Execution",
            detail: "No strategy configured",
            state: "pending",
        },
        storage: StatusItem {
            title: "Trade storage",
            detail: "Not configured",
            state: "pending",
        },
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let web_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web");
    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/status", get(status))
        .fallback_service(ServeDir::new(web_dir).append_index_html_on_directories(true));

    let address: SocketAddr = "127.0.0.1:8080".parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("Dashboard running at http://{address}");
    axum::serve(listener, app).await?;
    Ok(())
}
