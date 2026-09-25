mod api;
mod backtest;
mod auth;
mod collector;
mod downloader;
mod market;
mod paper;
mod storage;
mod trading;

use axum::{Router, middleware};
use market::MarketService;
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tower_http::services::ServeDir;

const DEFAULT_DATABASE_PATH: &str = "data/binance_bots.sqlite3";
const DEFAULT_PORT: u16 = 8080;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--capture-live") {
        let options = collector::CaptureOptions::from_args(&args[1..])?;
        collector::run(options).await?;
        return Ok(());
    }

    let market_service = Arc::new(MarketService::new());
    let storage_reader = Arc::new(storage::StorageReader::new(database_path()));
    storage_reader.initialize()?;
    let auth_state = Arc::new(auth::AuthState::from_env(Arc::clone(&storage_reader))?);
    let paper_manager = paper::PaperManager::new(Arc::clone(&storage_reader), Arc::clone(&market_service))?;
    let web_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web");

    let protected_app = api::protected_router(market_service, storage_reader, paper_manager)
        .fallback_service(ServeDir::new(web_dir).append_index_html_on_directories(true))
        .layer(middleware::from_fn_with_state(
            Arc::clone(&auth_state),
            auth::require_auth,
        ));

    let app = Router::new()
        .route("/api/health", axum::routing::get(api::health))
        .merge(auth::public_router(auth_state))
        .merge(protected_app);

    let address = server_address()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("Dashboard running at http://{address}");
    axum::serve(listener, app).await?;
    Ok(())
}

fn database_path() -> PathBuf {
    std::env::var_os("BINANCE_BOTS_DATABASE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_DATABASE_PATH))
}

fn server_address() -> Result<SocketAddr, std::num::ParseIntError> {
    let render_port = std::env::var("PORT").ok();
    let port = match render_port.as_deref() {
        Some(value) => value.parse::<u16>()?,
        None => DEFAULT_PORT,
    };

    let address = if render_port.is_some() {
        SocketAddr::from(([0, 0, 0, 0], port))
    } else {
        SocketAddr::from(([127, 0, 0, 1], port))
    };
    Ok(address)
}
