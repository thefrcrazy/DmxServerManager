use axum::{
    routing::get,
    Router,
};
use crate::core::AppState;

pub mod auth;
pub mod backups;
pub mod collaboration;
pub mod console;
pub mod filesystem;
pub mod metrics;
pub mod roles;
pub mod servers;
pub mod settings;
pub mod setup;
pub mod system;
pub mod upload;
pub mod users;
pub mod webhook;

pub fn routes() -> Router<AppState> {
    Router::new()
        .nest("/auth", auth::routes())
        .nest("/backups", backups::routes())
        .nest("/collaboration", collaboration::routes())
        .nest("/filesystem", filesystem::routes())
        .nest("/servers", servers::routes()) // servers::routes() now includes metrics merging inside it if kept consistent
        .nest("/settings", settings::routes())
        .nest("/setup", setup::routes())
        .nest("/roles", roles::routes())
        .nest("/system", system::routes())
        .nest("/upload", upload::routes())
        .nest("/users", users::routes())
        .nest("/webhook", webhook::routes())
        .route("/ws/console/:id", get(console::ws_handler))
}