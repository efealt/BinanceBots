mod api;
mod market;

use market::MarketService;
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let market_service = Arc::new(MarketService::new());
    let web_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web");
    let app = api::router(market_service)
        .fallback_service(ServeDir::new(web_dir).append_index_html_on_directories(true));

    let address: SocketAddr = "127.0.0.1:8080".parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("Dashboard running at http://{address}");
    axum::serve(listener, app).await?;
    Ok(())
}
