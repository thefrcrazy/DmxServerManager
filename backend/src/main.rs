use axum::{
    routing::{get_service},
    Router,
};
use tower_http::{
    cors::{CorsLayer, Any},
    services::ServeDir,
    trace::TraceLayer,
};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

// Modules will be uncommented as they are migrated
mod api;
mod config;
mod db;
mod error;
mod models;
mod services;
mod templates;
mod utils;

use config::Settings;
use services::ProcessManager;
use db::DbPool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub pool: DbPool,
    pub process_manager: ProcessManager,
    pub settings: Arc<Settings>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load configuration
    dotenvy::dotenv().ok();
    let settings = Settings::from_env();

    // Ensure data directory exists
    std::fs::create_dir_all("data").ok();
    std::fs::create_dir_all(&settings.uploads_dir).ok();

    info!("🚀 Draveur Manager v{}", env!("CARGO_PKG_VERSION"));
    info!("📡 Starting server on {}:{}", settings.host, settings.port);

    // Initialize database
    let pool = db::init_pool(&settings.database_url).await?;
    db::run_migrations(&pool).await?;

    // Initialize services
    let process_manager = ProcessManager::new(Some(pool.clone()));

    // Start background services
    services::scheduler::start(pool.clone(), process_manager.clone());

    let state = AppState {
        pool,
        process_manager,
        settings: Arc::new(settings.clone()),
    };
    
    let uploads_dir = settings.uploads_dir.clone();

    // CORS configuration
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .nest("/api/v1", api::routes())
        
        // Serve uploaded files
        .nest_service("/uploads", get_service(ServeDir::new(&uploads_dir)))
        
        // Serve frontend in production (static files)
        .nest_service("/", get_service(ServeDir::new("./static")))
        
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state);

    let addr = format!("{}:{}", settings.host, settings.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    
    axum::serve(listener, app).await?;

    Ok(())
}
