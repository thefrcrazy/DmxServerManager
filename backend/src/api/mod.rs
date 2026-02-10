use axum::{
    routing::get,
    Json, Router,
};
use serde::Serialize;
use crate::core::AppState;

/// Réponse standard pour les opérations réussies
#[derive(Debug, Serialize)]
pub struct SuccessResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl SuccessResponse {
    pub fn ok() -> Json<Self> {
        Json(Self { success: true, message: None })
    }

    pub fn with_message(msg: impl Into<String>) -> Json<Self> {
        Json(Self { success: true, message: Some(msg.into()) })
    }
}

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