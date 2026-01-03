//! RS-Proxy: A reverse proxy for injecting thinking configuration into API requests.
//!
//! This proxy parses model name suffixes (e.g., `model(high)` or `model(16384)`)
//! and injects corresponding thinking configuration into API requests.

mod config;
mod error;
mod models;
mod protocol;
mod proxy;
mod thinking;

use config::Args;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Initialize tracing with environment filter.
fn init_tracing() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rs_proxy=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
}

#[tokio::main]
async fn main() {
    // Initialize tracing
    init_tracing();

    // Parse command line arguments
    let args = Args::parse();

    tracing::info!("RS-Proxy starting...");
    tracing::info!("Listening on port: {}", args.port);
    tracing::info!("Upstream URL: {}", args.upstream_url());

    // TODO: Implement server startup in future tasks
    // For now, just verify that the modules compile correctly
    tracing::info!("RS-Proxy modules loaded successfully");

    // Create HTTP client
    let _client = proxy::create_client();
    tracing::info!("HTTP client created");

    // Verify model registry is accessible
    if let Some(model) = models::get_model_info("claude-sonnet-4-5-20250929") {
        tracing::info!("Model registry loaded, sample model: {}", model.id);
    }

    tracing::info!("RS-Proxy initialization complete");
}
