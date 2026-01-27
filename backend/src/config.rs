/// Application settings loaded from environment variables
pub struct Settings {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub uploads_dir: String,
}

impl Settings {
    pub fn from_env() -> Self {
        Self {
            host: std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into()),
            port: std::env::var("PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(5500),
            database_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite:data/draveur.db?mode=rwc".into()),
            uploads_dir: std::env::var("UPLOADS_DIR").unwrap_or_else(|_| "./data/uploads".into()),
        }
    }
}
