use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};

use crate::{
    api::auth::{AuthUser, authorize_instance},
    core::{AppState, error::AppError},
    services::players,
};

pub fn routes() -> Router<AppState> {
    Router::new().route("/servers/{id}/players", get(snapshot))
}

async fn snapshot(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(instance_id): Path<String>,
) -> Result<Json<players::PlayerSnapshot>, AppError> {
    super::servers::validate_instance_id(&instance_id)?;
    authorize_instance(&state, &auth, &instance_id, "server.read").await?;
    let profile_id: String = sqlx::query_scalar("SELECT profile_id FROM instances WHERE id = ?")
        .bind(&instance_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("servers.not_found".into()))?;
    players::snapshot(&state.pool, &instance_id, &profile_id)
        .await
        .map(Json)
}
