mod api;
mod collector;
mod downloader;
mod market;
mod storage;

use market::MarketService;
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tower_http::services::ServeDir;

const DEFAULT_DATABASE_PATH: &str = "data/binance_grid.sqlite3";
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
    let web_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web");
    let app = api::router(market_service, storage_reader)
        .fallback_service(ServeDir::new(web_dir).append_index_html_on_directories(true));

    let address = server_address()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("Dashboard running at http://{address}");
    axum::serve(listener, app).await?;
    Ok(())
}

fn database_path() -> PathBuf {
    std::env::var_os("BINANCE_GRID_DATABASE_PATH")
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
