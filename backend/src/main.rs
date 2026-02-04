use axum::{
    routing::{get_service},
    Router,
};
use tower_http::{
    cors::CorsLayer,
    services::ServeDir,
    trace::TraceLayer,
};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

// Module definitions
mod api;
mod core;    // New Core module (Config, DB, Errors)
mod domain;  // New Domain module (Models)
mod services;
mod utils;

use core::{Settings, AppState};
use services::game::ProcessManager;
use core::database;
use std::sync::Arc;

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
    let pool = database::init_pool(&settings.database_url).await?;
    database::run_migrations(&pool).await?;

    // Initialize services
    let process_manager = ProcessManager::new(Some(pool.clone()));

    // Start background services
    services::system::scheduler::start(pool.clone(), process_manager.clone());

    let state = AppState {
        pool,
        process_manager,
        settings: Arc::new(settings.clone()),
    };
    
    let uploads_dir = settings.uploads_dir.clone();

    // CORS configuration
    let mut allowed_origins: Vec<axum::http::HeaderValue> = vec![
        "http://localhost:5173".parse().unwrap(), // Vite dev server
        "http://localhost:5500".parse().unwrap(), // Backend serving frontend
        "http://127.0.0.1:5173".parse().unwrap(),
        "http://127.0.0.1:5500".parse().unwrap(),
    ];
    
    // Add custom frontend URL from environment
    if let Ok(frontend_url) = std::env::var("FRONTEND_URL") {
        if let Ok(origin) = frontend_url.parse() {
            allowed_origins.push(origin);
            info!("🔒 Added CORS origin: {}", frontend_url);
        }
    }
    
    let cors = CorsLayer::new()
        .allow_origin(allowed_origins)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PUT,
            axum::http::Method::PATCH,
            axum::http::Method::DELETE,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
            axum::http::header::HeaderName::from_static("x-requested-with"),
            axum::http::header::HeaderName::from_static("accept"),
            axum::http::header::HeaderName::from_static("origin"),
        ])
        .allow_credentials(true);

    let app = Router::new()
        .nest("/api/v1", api::routes())
        
        // Serve uploaded files
        .nest_service("/uploads", get_service(ServeDir::new(&uploads_dir)))
        
        // Serve frontend in production (static files)
        .nest_service("/", get_service(
            ServeDir::new("./static")
                .fallback(tower_http::services::ServeFile::new("./static/index.html"))
        ))
        
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state);

    let addr = format!("{}:{}", settings.host, settings.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    
    axum::serve(listener, app).await?;

    Ok(())
}
