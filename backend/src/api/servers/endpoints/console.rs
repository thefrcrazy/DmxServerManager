use axum::{
    extract::{Path, State},
    Json,
};
use crate::core::AppState;
use crate::core::error::AppError;
use crate::api::servers::models::CommandRequest;

pub async fn send_command(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CommandRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    state.process_manager.send_command(&id, &body.command).await?;
    Ok(Json(serde_json::json!({ "success": true })))
}
