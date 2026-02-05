use axum::{
    routing::{get, post, put},
    Router,
};
use crate::core::AppState;

use super::endpoints::{crud, lifecycle, files, players, console, schedules};
use crate::api::metrics;

pub fn routes() -> Router<AppState> {
    Router::new()
        // Servers CRUD
        .route("/", get(crud::list_servers).post(crud::create_server))
        .route("/:id", get(crud::get_server).put(crud::update_server).delete(crud::delete_server))
        
        // Actions
        .route("/:id/start", post(lifecycle::start_server))
        .route("/:id/stop", post(lifecycle::stop_server))
        .route("/:id/restart", post(lifecycle::restart_server))
        .route("/:id/kill", post(lifecycle::kill_server))
        .route("/:id/reinstall", post(lifecycle::reinstall_server))
        .route("/:id/command", post(console::send_command))
        
        // Files API
        .route("/:id/files", get(files::list_server_files))
        .route("/:id/files/read", get(files::read_server_file))
        .route("/:id/files/write", post(files::write_server_file))
        .route("/:id/files/delete", post(files::delete_server_file))
        .route("/:id/files/mkdir", post(files::create_folder))
        .route("/:id/files/create", post(files::create_file))
        .route("/:id/files/upload", post(files::upload_file))
        .route("/:id/files/download", get(files::download_file))
        .route("/:id/files/rename", post(files::rename_file))
        .route("/:id/files/copy", post(files::copy_file))
        .route("/:id/files/move", post(files::move_file))
        
        // Players API
        .route("/:id/whitelist", get(players::get_whitelist).post(players::add_whitelist).delete(players::remove_whitelist))
        .route("/:id/bans", get(players::get_bans).post(players::add_ban).delete(players::remove_ban))
        .route("/:id/ops", get(players::get_ops).post(players::add_op).delete(players::remove_op))
        
        // Schedules API
        .route("/:id/schedules", get(schedules::list_schedules).post(schedules::create_schedule))
        .route("/:id/schedules/:schedule_id", put(schedules::update_schedule).delete(schedules::delete_schedule))
        .route("/:id/schedules/:schedule_id/toggle", post(schedules::toggle_schedule))
        .route("/:id/schedules/:schedule_id/run", post(schedules::run_schedule))
        
        // Metrics merging (retained from original mod.rs)
        .merge(metrics::routes())
}
