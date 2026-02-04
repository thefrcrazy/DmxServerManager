use std::fs;
use std::path::Path;
use rcgen::generate_simple_self_signed;
use tracing::{info, warn};

/// Ensures that a self-signed certificate exists, generates it if not.
pub fn ensure_self_signed_certs() -> anyhow::Result<(String, String)> {
    let cert_path = "data/cert.pem";
    let key_path = "data/key.pem";

    if Path::new(cert_path).exists() && Path::new(key_path).exists() {
        info!("🔐 Existing TLS certificates found in data/");
        return Ok((cert_path.to_string(), key_path.to_string()));
    }

    warn!("🛡️ Generating new self-signed TLS certificates...");

    // Generate for localhost and any IP
    let subject_alt_names = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    let cert = generate_simple_self_signed(subject_alt_names)?;
    
    fs::write(cert_path, cert.cert.pem())?;
    fs::write(key_path, cert.key_pair.serialize_pem())?;

    info!("✅ Self-signed certificates generated successfully in data/");
    Ok((cert_path.to_string(), key_path.to_string()))
}
