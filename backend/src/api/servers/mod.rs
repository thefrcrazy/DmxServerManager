use axum::{
    routing::{get, post},
    Router,
};
use crate::AppState;

pub mod handlers;
pub mod models;
pub mod files;

use handlers::*;
use files::*;

pub fn routes() -> Router<AppState> {
    Router::new()
        // Servers CRUD
        .route("/", get(list_servers).post(create_server))
        .route("/:id", get(get_server).put(update_server).delete(delete_server))
        
        // Actions
        .route("/:id/start", post(start_server))
        .route("/:id/stop", post(stop_server))
        .route("/:id/restart", post(restart_server))
        .route("/:id/kill", post(kill_server))
        .route("/:id/reinstall", post(reinstall_server))
        .route("/:id/command", post(send_command))
        
        // Files API
        .route("/:id/files", get(list_server_files))
        .route("/:id/files/read", get(read_server_file))
        .route("/:id/files/write", post(write_server_file))
        .route("/:id/files/delete", post(delete_server_file))
}
