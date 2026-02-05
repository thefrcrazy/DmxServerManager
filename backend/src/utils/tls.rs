use std::fs;
use std::path::Path;
use rcgen::generate_simple_self_signed;
use tracing::{info, warn};

/// Ensures that a self-signed certificate exists, generates it if not.
pub fn ensure_self_signed_certs() -> anyhow::Result<(String, String)> {
    let is_docker = std::env::var("IS_DOCKER").unwrap_or_else(|_| "false".into()) == "true";
    let base_path = if is_docker { "/data" } else { "data" };
    
    let cert_path = format!("{base_path}/cert.pem");
    let key_path = format!("{base_path}/key.pem");

    if Path::new(&cert_path).exists() && Path::new(&key_path).exists() {
        info!("🔐 Existing TLS certificates found in {}", base_path);
        return Ok((cert_path, key_path));
    }

    warn!("🛡️ Generating new self-signed TLS certificates in {}...", base_path);

    // Ensure directory exists
    if !Path::new(base_path).exists() {
        fs::create_dir_all(base_path)?;
    }

    // Generate for localhost and any IP
    let subject_alt_names = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    let cert = generate_simple_self_signed(subject_alt_names)?;
    
    fs::write(&cert_path, cert.cert.pem())?;
    fs::write(&key_path, cert.key_pair.serialize_pem())?;

    info!("✅ Self-signed certificates generated successfully in {}", base_path);
    Ok((cert_path, key_path))
}
