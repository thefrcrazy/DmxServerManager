use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};

use crate::{
    api::auth::AuthUser,
    core::{AppState, database, error::AppError},
    services::releases::{ReleaseCheckState, ReleaseStatus},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/releases/panel", get(status))
        .route("/releases/panel/check", post(check))
}

async fn status(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<ReleaseStatus>, AppError> {
    require_owner(&auth)?;
    Ok(Json(state.releases.status().await))
}

async fn check(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<ReleaseStatus>, AppError> {
    require_owner(&auth)?;
    let status = state.releases.check_now().await;
    let outcome = if status.state == ReleaseCheckState::CheckFailed {
        "failed"
    } else {
        "success"
    };
    database::audit(
        &state.pool,
        Some(&auth.id),
        "panel_release.check",
        "panel_release",
        None,
        outcome,
        serde_json::json!({
            "state": status.state,
            "error_code": status.error_code,
        }),
    )
    .await?;
    Ok(Json(status))
}

fn require_owner(auth: &AuthUser) -> Result<(), AppError> {
    if auth.role == "owner" {
        Ok(())
    } else {
        Err(AppError::Forbidden("releases.owner_required".into()))
    }
}
