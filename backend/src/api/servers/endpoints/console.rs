use axum::{
    extract::{Path, State},
    Json,
};

use crate::core::AppState;
use crate::api::SuccessResponse;
use crate::core::error::AppError;
use crate::api::servers::models::CommandRequest;

pub async fn send_command(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CommandRequest>,
) -> Result<Json<SuccessResponse>, AppError> {
    state.process_manager.send_command(&id, &body.command).await?;
    Ok(SuccessResponse::ok())
}
