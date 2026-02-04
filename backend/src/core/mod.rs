pub mod config;
pub mod database;
pub mod error;

pub use config::Settings;
pub use database::DbPool;
pub use error::AppError;

use crate::services::game::ProcessManager;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub pool: DbPool,
    pub process_manager: ProcessManager,
    pub settings: Arc<Settings>,
}
