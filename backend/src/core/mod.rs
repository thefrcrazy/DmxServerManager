pub mod config;
pub mod database;
pub mod error;
pub mod events;

pub use config::Settings;
pub use database::DbPool;

use crate::services::metrics::SystemMetricsService;
use crate::services::profiles::ProfileRegistry;
use crate::services::releases::ReleaseMonitor;
use crate::services::runtime::RuntimeManager;
use crate::services::secrets::SecretStore;
use events::EventHub;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub pool: DbPool,
    pub settings: Arc<Settings>,
    pub profiles: Arc<ProfileRegistry>,
    pub events: EventHub,
    pub secrets: SecretStore,
    pub runtime: RuntimeManager,
    pub releases: ReleaseMonitor,
    pub system_metrics: SystemMetricsService,
}
