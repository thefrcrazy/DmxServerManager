use actix_cors::Cors;
use actix_files::{Files, NamedFile};
use actix_web::{middleware, web, App, HttpServer};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod api;
mod config;
mod db;
mod error;
mod models;
mod services;
mod templates;

use config::Settings;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
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

    info!("🚀 Kweebec Manager v{}", env!("CARGO_PKG_VERSION"));
    info!("📡 Starting server on {}:{}", settings.host, settings.port);

    // Initialize database
    let pool = db::init_pool(&settings.database_url).await?;
    db::run_migrations(&pool).await?;

    // Initialize services
    let process_manager = web::Data::new(services::ProcessManager::new(Some(pool.clone())));

    // Start background services
    services::scheduler::start(web::Data::new(pool.clone()), process_manager.clone());

    let uploads_dir = settings.uploads_dir.clone();
    HttpServer::new(move || {
        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header()
            .max_age(3600);

        App::new()
            .wrap(cors)
            .wrap(middleware::Logger::default())
            .app_data(web::Data::new(pool.clone()))
            .app_data(process_manager.clone())
            // API routes
            .service(
                web::scope("/api/v1")
                    .configure(api::auth::configure)
                    .configure(api::servers::configure)
                    .configure(api::backups::configure)
                    .configure(api::settings::configure)
                    .configure(api::system::configure)
                    .configure(api::users::configure)
                    .configure(api::filesystem::configure)
                    .configure(api::webhook::configure)
                    .configure(api::upload::configure)
                    .configure(api::setup::configure),
            )
            // WebSocket for console
            .route("/ws/console/{server_id}", web::get().to(api::console::ws_handler))
            // Serve uploaded files
            .service(Files::new("/uploads", &uploads_dir))
            // Serve frontend in production
            .service(
                Files::new("/", "./static")
                    .index_file("index.html")
                    .default_handler(web::get().to(|| async {
                        NamedFile::open("./static/index.html")
                    })),
            )
    })
    .bind(format!("{}:{}", settings.host, settings.port))?
    .run()
    .await
}
