mod api;
mod collector;
mod market;
mod storage;

use market::MarketService;
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--capture-live") {
        let options = collector::CaptureOptions::from_args(&args[1..])?;
        collector::run(options).await?;
        return Ok(());
    }

    let market_service = Arc::new(MarketService::new());
    let storage_reader = Arc::new(storage::StorageReader::new(PathBuf::from(
        "data/binance_grid.sqlite3",
    )));
    storage_reader.initialize()?;
    let web_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web");
    let app = api::router(market_service, storage_reader)
        .fallback_service(ServeDir::new(web_dir).append_index_html_on_directories(true));

    let address: SocketAddr = "127.0.0.1:8080".parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("Dashboard running at http://{address}");
    axum::serve(listener, app).await?;
    Ok(())
}
