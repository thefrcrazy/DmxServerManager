use std::{path::Path, sync::Arc};

#[cfg(any(not(windows), test))]
use std::{fs::OpenOptions, io::Write};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use rand::{RngCore, rngs::OsRng};

use crate::{
    core::{DbPool, error::AppError},
    domain::v1::GameProfile,
};

#[derive(Clone)]
pub struct SecretStore {
    key: Arc<[u8; 32]>,
}

impl SecretStore {
    pub fn load_or_create(path: &Path) -> anyhow::Result<Self> {
        let key = if path.exists() {
            validate_permissions(path)?;
            decode_key(&std::fs::read(path)?)?
        } else {
            #[cfg(all(windows, not(test)))]
            anyhow::bail!(
                "master key is missing; provision it with the Windows installer so restrictive ACLs are applied"
            );
            #[cfg(any(not(windows), test))]
            {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut key = [0_u8; 32];
                OsRng.fill_bytes(&mut key);
                create_key_file(path, &key)?;
                key
            }
        };
        Ok(Self { key: Arc::new(key) })
    }

    pub async fn set(
        &self,
        pool: &DbPool,
        instance_id: &str,
        name: &str,
        value: &str,
    ) -> Result<(), AppError> {
        validate_secret_name(name)?;
        if value.is_empty() || value.len() > 16 * 1024 {
            return Err(AppError::BadRequest("secrets.invalid_value".into()));
        }
        let associated_data = format!("{instance_id}:{name}");
        let (nonce, ciphertext) = self.seal(&associated_data, value)?;
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO instance_secrets
                (instance_id, name, nonce, ciphertext, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(instance_id, name) DO UPDATE SET
                nonce = excluded.nonce,
                ciphertext = excluded.ciphertext,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(instance_id)
        .bind(name)
        .bind(nonce)
        .bind(ciphertext)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn get(
        &self,
        pool: &DbPool,
        instance_id: &str,
        name: &str,
    ) -> Result<Option<String>, AppError> {
        let row: Option<(String, String)> = sqlx::query_as(
            "SELECT nonce, ciphertext FROM instance_secrets WHERE instance_id = ? AND name = ?",
        )
        .bind(instance_id)
        .bind(name)
        .fetch_optional(pool)
        .await?;
        let Some((nonce, ciphertext)) = row else {
            return Ok(None);
        };
        let associated_data = format!("{instance_id}:{name}");
        self.open(&associated_data, &nonce, &ciphertext).map(Some)
    }

    pub(crate) fn seal(
        &self,
        associated_data: &str,
        value: &str,
    ) -> Result<(String, String), AppError> {
        if value.is_empty() || value.len() > 16 * 1024 {
            return Err(AppError::BadRequest("secrets.invalid_value".into()));
        }
        let cipher = XChaCha20Poly1305::new(self.key.as_ref().into());
        let mut nonce = [0_u8; 24];
        OsRng.fill_bytes(&mut nonce);
        let ciphertext = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: value.as_bytes(),
                    aad: associated_data.as_bytes(),
                },
            )
            .map_err(|_| AppError::Internal("secret encryption failed".into()))?;
        Ok((
            URL_SAFE_NO_PAD.encode(nonce),
            URL_SAFE_NO_PAD.encode(ciphertext),
        ))
    }

    pub(crate) fn open(
        &self,
        associated_data: &str,
        nonce: &str,
        ciphertext: &str,
    ) -> Result<String, AppError> {
        let nonce = URL_SAFE_NO_PAD
            .decode(nonce)
            .map_err(|_| AppError::Internal("stored secret nonce is invalid".into()))?;
        let ciphertext = URL_SAFE_NO_PAD
            .decode(ciphertext)
            .map_err(|_| AppError::Internal("stored secret is invalid".into()))?;
        if nonce.len() != 24 {
            return Err(AppError::Internal("stored secret nonce is invalid".into()));
        }
        let cipher = XChaCha20Poly1305::new(self.key.as_ref().into());
        let plaintext = cipher
            .decrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: associated_data.as_bytes(),
                },
            )
            .map_err(|_| AppError::Internal("secret decryption failed".into()))?;
        String::from_utf8(plaintext)
            .map_err(|_| AppError::Internal("stored secret is not UTF-8".into()))
    }
}

pub fn allowed_secret_names(profile_id: &str) -> &'static [&'static str] {
    match profile_id {
        "valheim" => &["server_password"],
        "palworld" => &["server_password", "admin_password"],
        "seven-days-to-die" => &["server_password"],
        "project-zomboid" => &["admin_password"],
        "rust" => &["rcon_password"],
        _ => &[],
    }
}

pub fn required_secret_names(profile_id: &str) -> &'static [&'static str] {
    match profile_id {
        "valheim" => &["server_password"],
        "project-zomboid" => &["admin_password"],
        "rust" => &["rcon_password"],
        _ => &[],
    }
}

pub fn validate_profile_secret(profile_id: &str, name: &str) -> Result<(), AppError> {
    if allowed_secret_names(profile_id).contains(&name) {
        Ok(())
    } else {
        Err(AppError::BadRequest(
            "secrets.not_supported_by_profile".into(),
        ))
    }
}

pub fn validate_profile_secret_value(
    profile: &GameProfile,
    name: &str,
    value: &str,
) -> Result<(), AppError> {
    validate_profile_secret(&profile.id, name)?;
    let property = profile
        .settings_schema
        .get("properties")
        .and_then(|properties| properties.get(name))
        .ok_or_else(|| AppError::BadRequest("secrets.not_supported_by_profile".into()))?;
    if property.get("type").and_then(serde_json::Value::as_str) != Some("string")
        || property.get("secret").and_then(serde_json::Value::as_bool) != Some(true)
        || property
            .get("writeOnly")
            .and_then(serde_json::Value::as_bool)
            != Some(true)
    {
        return Err(AppError::BadRequest(
            "secrets.not_supported_by_profile".into(),
        ));
    }
    let length = value.chars().count() as u64;
    let below_minimum = property
        .get("minLength")
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|minimum| length < minimum);
    let above_maximum = property
        .get("maxLength")
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|maximum| length > maximum);
    if value.is_empty()
        || value.len() > 16 * 1024
        || below_minimum
        || above_maximum
        || value.chars().any(char::is_control)
    {
        return Err(AppError::BadRequest("secrets.invalid_value".into()));
    }
    Ok(())
}

fn validate_secret_name(name: &str) -> Result<(), AppError> {
    if !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        Ok(())
    } else {
        Err(AppError::BadRequest("secrets.invalid_name".into()))
    }
}

fn decode_key(bytes: &[u8]) -> anyhow::Result<[u8; 32]> {
    let decoded = if bytes.len() == 32 {
        bytes.to_vec()
    } else {
        URL_SAFE_NO_PAD.decode(String::from_utf8_lossy(bytes).trim())?
    };
    decoded
        .try_into()
        .map_err(|_| anyhow::anyhow!("master key must contain exactly 32 bytes"))
}

#[cfg(any(not(windows), test))]
fn create_key_file(path: &Path, contents: &[u8]) -> anyhow::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(contents)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(unix)]
fn validate_permissions(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path)?.permissions().mode();
    let docker_secret = path.starts_with("/run/secrets") && mode_is_docker_secret(mode);
    if !docker_secret && !mode_is_native_secret(mode) {
        anyhow::bail!(
            "master key file {} must be 0600/0640, or a read-only /run/secrets mount",
            path.display()
        );
    }
    Ok(())
}

#[cfg(unix)]
fn mode_is_native_secret(mode: u32) -> bool {
    // Owner read is mandatory. Group read is supported for root:service installations,
    // but group write/execute and every permission for other users are rejected.
    mode & 0o400 != 0 && mode & 0o027 == 0
}

#[cfg(unix)]
fn mode_is_docker_secret(mode: u32) -> bool {
    // Docker secrets are commonly mounted 0444. They must remain entirely read-only.
    mode & 0o444 != 0 && mode & 0o333 == 0
}

#[cfg(not(unix))]
fn validate_permissions(_path: &Path) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_names_are_closed_per_profile() {
        assert!(validate_profile_secret("valheim", "server_password").is_ok());
        assert!(validate_profile_secret("valheim", "steam_password").is_err());
        assert!(validate_profile_secret("rust", "rcon_password").is_ok());
        assert!(validate_profile_secret("project-zomboid", "admin_password").is_ok());
        assert!(validate_profile_secret("steam-custom", "password").is_err());
    }

    #[test]
    fn profile_secret_values_apply_the_declared_schema_constraints() {
        let profiles = crate::services::profiles::ProfileRegistry::builtins();
        let valheim = profiles.get("valheim").unwrap();
        assert!(validate_profile_secret_value(&valheim, "server_password", "abcde").is_ok());
        assert!(validate_profile_secret_value(&valheim, "server_password", "abcd").is_err());
        assert!(
            validate_profile_secret_value(&valheim, "server_password", &"a".repeat(65)).is_err()
        );
        assert!(
            validate_profile_secret_value(&valheim, "server_password", "safe\nunsafe").is_err()
        );

        let palworld = profiles.get("palworld").unwrap();
        assert!(validate_profile_secret_value(&palworld, "admin_password", "x").is_ok());
        assert!(validate_profile_secret_value(&palworld, "unknown", "value").is_err());
    }

    #[test]
    fn key_decoder_accepts_raw_and_base64_keys() {
        let raw = [7_u8; 32];
        assert_eq!(decode_key(&raw).unwrap(), raw);
        assert_eq!(
            decode_key(URL_SAFE_NO_PAD.encode(raw).as_bytes()).unwrap(),
            raw
        );
    }

    #[test]
    fn newly_provisioned_keys_use_exactly_32_raw_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("master.key");

        let _store = SecretStore::load_or_create(&path).unwrap();

        assert_eq!(std::fs::read(path).unwrap().len(), 32);
    }

    #[cfg(unix)]
    #[test]
    fn accepts_native_and_docker_secret_modes_but_rejects_writable_keys() {
        assert!(mode_is_native_secret(0o100600));
        assert!(mode_is_native_secret(0o100640));
        assert!(!mode_is_native_secret(0o100644));
        assert!(mode_is_docker_secret(0o100444));
        assert!(!mode_is_docker_secret(0o100666));
    }
}
