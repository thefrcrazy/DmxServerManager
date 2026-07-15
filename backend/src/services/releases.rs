use std::{sync::Arc, time::Duration};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, VerifyingKey};
use futures::StreamExt;
use reqwest::{Client, Url, redirect::Policy};
use semver::Version;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock, oneshot};
use tracing::warn;

use crate::core::config::{DeploymentMode, Settings, is_safe_release_url};

const MAX_ENVELOPE_BYTES: usize = 256 * 1024;
const MAX_PAYLOAD_BYTES: usize = 128 * 1024;
const RELEASE_IMAGE: &str = "ghcr.io/thefrcrazy/dmx-server-manager";

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseCheckState {
    Disabled,
    NeverChecked,
    Checking,
    UpToDate,
    UpdateAvailable,
    CheckFailed,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseCheckErrorCode {
    Network,
    ResponseTooLarge,
    EnvelopeInvalid,
    SignatureInvalid,
    ManifestInvalid,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReleaseTarget {
    Native {
        platform: NativePlatform,
        archive_url: String,
        archive_sha256: String,
        installer_url: String,
        installer_sha256: String,
        upgrade_command: String,
    },
    Docker {
        image: String,
        digest: String,
        pull_command: String,
        apply_command: String,
    },
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum NativePlatform {
    LinuxAmd64,
    WindowsAmd64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct VerifiedPanelRelease {
    pub version: String,
    pub published_at: String,
    pub notes_url: String,
    pub target: ReleaseTarget,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ReleaseStatus {
    pub configured: bool,
    pub current_version: String,
    pub deployment_mode: DeploymentMode,
    pub state: ReleaseCheckState,
    pub checked_at: Option<String>,
    pub latest: Option<VerifiedPanelRelease>,
    pub error_code: Option<ReleaseCheckErrorCode>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedEnvelope {
    payload: String,
    signature: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseManifest {
    schema_version: u8,
    version: String,
    published_at: String,
    notes_url: String,
    native: NativeArtifacts,
    docker: DockerArtifact,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeArtifacts {
    linux_amd64: NativeArtifact,
    windows_amd64: NativeArtifact,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeArtifact {
    archive_url: String,
    archive_sha256: String,
    installer_url: String,
    installer_sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DockerArtifact {
    image: String,
    digest: String,
}

#[derive(Clone)]
pub struct ReleaseMonitor {
    settings: Arc<Settings>,
    client: Client,
    status: Arc<RwLock<ReleaseStatus>>,
    check_lock: Arc<Mutex<()>>,
}

impl ReleaseMonitor {
    pub fn new(settings: Arc<Settings>) -> anyhow::Result<Self> {
        let configured = settings.release_check.is_some();
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .redirect(Policy::custom(|attempt| {
                if attempt.previous().len() >= 3 {
                    return attempt.error("release manifest redirect limit exceeded");
                }
                if is_safe_release_url(attempt.url(), true) {
                    attempt.follow()
                } else {
                    attempt.error("unsafe release manifest redirect")
                }
            }))
            .user_agent(concat!(
                "DmxServerManager/",
                env!("CARGO_PKG_VERSION"),
                " release-monitor"
            ))
            .build()?;
        Ok(Self {
            status: Arc::new(RwLock::new(ReleaseStatus {
                configured,
                current_version: env!("CARGO_PKG_VERSION").into(),
                deployment_mode: settings.deployment_mode,
                state: if configured {
                    ReleaseCheckState::NeverChecked
                } else {
                    ReleaseCheckState::Disabled
                },
                checked_at: None,
                latest: None,
                error_code: None,
            })),
            settings,
            client,
            check_lock: Arc::new(Mutex::new(())),
        })
    }

    pub async fn status(&self) -> ReleaseStatus {
        self.status.read().await.clone()
    }

    pub async fn check_now(&self) -> ReleaseStatus {
        let _guard = self.check_lock.lock().await;
        if self.settings.release_check.is_none() {
            return self.status().await;
        }
        {
            let mut status = self.status.write().await;
            status.state = ReleaseCheckState::Checking;
            status.error_code = None;
        }
        let checked_at = Utc::now().to_rfc3339();
        let result = self.fetch_verified_release().await;
        if let Err(error_code) = &result {
            warn!(?error_code, "signed panel release check failed");
        }
        let mut status = self.status.write().await;
        apply_check_result(&mut status, checked_at, result);
        drop(status);
        self.status().await
    }

    pub fn interval_seconds(&self) -> Option<u64> {
        self.settings
            .release_check
            .as_ref()
            .map(|config| config.interval_seconds)
    }

    async fn fetch_verified_release(&self) -> Result<VerifiedPanelRelease, ReleaseCheckErrorCode> {
        let config = self
            .settings
            .release_check
            .as_ref()
            .ok_or(ReleaseCheckErrorCode::ManifestInvalid)?;
        let response = self
            .client
            .get(config.manifest_url.clone())
            .send()
            .await
            .map_err(|_| ReleaseCheckErrorCode::Network)?;
        if !response.status().is_success() {
            return Err(ReleaseCheckErrorCode::Network);
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_ENVELOPE_BYTES as u64)
        {
            return Err(ReleaseCheckErrorCode::ResponseTooLarge);
        }
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| ReleaseCheckErrorCode::Network)?;
            if body.len().saturating_add(chunk.len()) > MAX_ENVELOPE_BYTES {
                return Err(ReleaseCheckErrorCode::ResponseTooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        verify_envelope_for_current(
            &body,
            config.public_key,
            self.settings.deployment_mode,
            env!("CARGO_PKG_VERSION"),
        )
    }
}

fn apply_check_result(
    status: &mut ReleaseStatus,
    checked_at: String,
    result: Result<VerifiedPanelRelease, ReleaseCheckErrorCode>,
) {
    match result {
        Ok(release) => {
            let update_available = Version::parse(&release.version)
                .ok()
                .zip(Version::parse(&status.current_version).ok())
                .is_some_and(|(latest, current)| latest > current);
            status.state = if update_available {
                ReleaseCheckState::UpdateAvailable
            } else {
                ReleaseCheckState::UpToDate
            };
            status.latest = Some(release);
            status.error_code = None;
        }
        Err(error_code) => {
            status.state = ReleaseCheckState::CheckFailed;
            status.latest = None;
            status.error_code = Some(error_code);
        }
    }
    status.checked_at = Some(checked_at);
}

pub struct ReleaseMonitorWorker {
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl ReleaseMonitorWorker {
    pub fn start(monitor: ReleaseMonitor) -> Self {
        let Some(interval_seconds) = monitor.interval_seconds() else {
            return Self {
                shutdown: None,
                task: None,
            };
        };
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(interval_seconds));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        monitor.check_now().await;
                    }
                    _ = &mut shutdown_rx => break,
                }
            }
        });
        Self {
            shutdown: Some(shutdown_tx),
            task: Some(task),
        }
    }

    pub async fn shutdown(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

fn verify_envelope(
    body: &[u8],
    public_key: [u8; 32],
    deployment_mode: DeploymentMode,
) -> Result<VerifiedPanelRelease, ReleaseCheckErrorCode> {
    let envelope: SignedEnvelope =
        serde_json::from_slice(body).map_err(|_| ReleaseCheckErrorCode::EnvelopeInvalid)?;
    if envelope.payload.len() > MAX_PAYLOAD_BYTES.saturating_mul(2)
        || envelope.signature.len() > 128
    {
        return Err(ReleaseCheckErrorCode::EnvelopeInvalid);
    }
    let payload = URL_SAFE_NO_PAD
        .decode(envelope.payload)
        .map_err(|_| ReleaseCheckErrorCode::EnvelopeInvalid)?;
    if payload.is_empty() || payload.len() > MAX_PAYLOAD_BYTES {
        return Err(ReleaseCheckErrorCode::EnvelopeInvalid);
    }
    let signature = URL_SAFE_NO_PAD
        .decode(envelope.signature)
        .ok()
        .and_then(|bytes| Signature::from_slice(&bytes).ok())
        .ok_or(ReleaseCheckErrorCode::SignatureInvalid)?;
    let verifying_key = VerifyingKey::from_bytes(&public_key)
        .map_err(|_| ReleaseCheckErrorCode::SignatureInvalid)?;
    if verifying_key.is_weak() || verifying_key.verify_strict(&payload, &signature).is_err() {
        return Err(ReleaseCheckErrorCode::SignatureInvalid);
    }
    let manifest: ReleaseManifest =
        serde_json::from_slice(&payload).map_err(|_| ReleaseCheckErrorCode::ManifestInvalid)?;
    validate_manifest(manifest, deployment_mode)
}

fn verify_envelope_for_current(
    body: &[u8],
    public_key: [u8; 32],
    deployment_mode: DeploymentMode,
    current_version: &str,
) -> Result<VerifiedPanelRelease, ReleaseCheckErrorCode> {
    let release = verify_envelope(body, public_key, deployment_mode)?;
    let latest =
        Version::parse(&release.version).map_err(|_| ReleaseCheckErrorCode::ManifestInvalid)?;
    let current =
        Version::parse(current_version).map_err(|_| ReleaseCheckErrorCode::ManifestInvalid)?;
    if latest < current {
        return Err(ReleaseCheckErrorCode::ManifestInvalid);
    }
    Ok(release)
}

fn validate_manifest(
    manifest: ReleaseManifest,
    deployment_mode: DeploymentMode,
) -> Result<VerifiedPanelRelease, ReleaseCheckErrorCode> {
    if manifest.schema_version != 1 {
        return Err(ReleaseCheckErrorCode::ManifestInvalid);
    }
    let version =
        Version::parse(&manifest.version).map_err(|_| ReleaseCheckErrorCode::ManifestInvalid)?;
    if version.to_string() != manifest.version || manifest.version.len() > 64 {
        return Err(ReleaseCheckErrorCode::ManifestInvalid);
    }
    DateTime::parse_from_rfc3339(&manifest.published_at)
        .map_err(|_| ReleaseCheckErrorCode::ManifestInvalid)?;
    let notes_url = parse_release_url(&manifest.notes_url)?;
    let expected_notes_suffix = format!("/releases/tag/v{}", manifest.version);
    if notes_url.host_str() != Some("github.com")
        || notes_url.path() != format!("/thefrcrazy/DmxServerManager{expected_notes_suffix}")
    {
        return Err(ReleaseCheckErrorCode::ManifestInvalid);
    }
    // Every target is validated even when it is not selected on this host. A
    // partially checksummed multi-platform manifest is never accepted.
    let linux_target = validate_native_target(
        manifest.native.linux_amd64,
        NativePlatform::LinuxAmd64,
        &manifest.version,
    )?;
    let windows_target = validate_native_target(
        manifest.native.windows_amd64,
        NativePlatform::WindowsAmd64,
        &manifest.version,
    )?;
    let docker_target = validate_docker_target(manifest.docker, &manifest.version)?;
    let target = match deployment_mode {
        DeploymentMode::Docker => docker_target,
        DeploymentMode::Native if cfg!(windows) => windows_target,
        DeploymentMode::Native => linux_target,
    };
    Ok(VerifiedPanelRelease {
        version: manifest.version,
        published_at: manifest.published_at,
        notes_url: notes_url.to_string(),
        target,
    })
}

fn validate_native_target(
    artifact: NativeArtifact,
    platform: NativePlatform,
    version: &str,
) -> Result<ReleaseTarget, ReleaseCheckErrorCode> {
    let archive_url = parse_release_url(&artifact.archive_url)?;
    let installer_url = parse_release_url(&artifact.installer_url)?;
    let archive_sha256 = validate_sha256(artifact.archive_sha256)?;
    let installer_sha256 = validate_sha256(artifact.installer_sha256)?;
    let upgrade_command = match platform {
        NativePlatform::LinuxAmd64 => format!(
            "p=$(mktemp /tmp/dmx-server-manager-install.XXXXXX) && trap 'rm -f \"$p\"' EXIT HUP INT TERM && curl --fail --location --proto '=https' --proto-redir '=https' --tlsv1.2 --output \"$p\" '{}' && printf '%s  %s\\n' '{}' \"$p\" | sha256sum --check --status && sudo DMX_VERSION='{}' DMX_EXPECTED_ARCHIVE_SHA256='{}' sh \"$p\"",
            installer_url, installer_sha256, version, archive_sha256
        ),
        NativePlatform::WindowsAmd64 => format!(
            "$p = Join-Path $env:TEMP ('dmx-server-manager-install-' + [guid]::NewGuid().ToString('N') + '.ps1'); try {{ Invoke-WebRequest -UseBasicParsing -Uri '{}' -OutFile $p; if ((Get-FileHash -Algorithm SHA256 $p).Hash.ToLowerInvariant() -cne '{}') {{ throw 'DmxServerManager installer checksum mismatch' }}; & $p -Version '{}' -ExpectedArchiveSha256 '{}' }} finally {{ Remove-Item -LiteralPath $p -Force -ErrorAction SilentlyContinue }}",
            installer_url, installer_sha256, version, archive_sha256
        ),
    };
    Ok(ReleaseTarget::Native {
        platform,
        archive_url: archive_url.to_string(),
        archive_sha256,
        installer_url: installer_url.to_string(),
        installer_sha256,
        upgrade_command,
    })
}

fn validate_docker_target(
    artifact: DockerArtifact,
    version: &str,
) -> Result<ReleaseTarget, ReleaseCheckErrorCode> {
    if artifact.image != RELEASE_IMAGE {
        return Err(ReleaseCheckErrorCode::ManifestInvalid);
    }
    let digest = artifact
        .digest
        .strip_prefix("sha256:")
        .map(str::to_owned)
        .ok_or(ReleaseCheckErrorCode::ManifestInvalid)
        .and_then(validate_sha256)?;
    let digest = format!("sha256:{digest}");
    let pinned_image = format!("{}@{}", artifact.image, digest);
    let authenticated_bootstrap = format!(
        "DMX_VERSION='{version}' DMX_IMAGE='{pinned_image}' sudo --preserve-env=DMX_VERSION,DMX_IMAGE ./bootstrap-docker.sh direct"
    );
    Ok(ReleaseTarget::Docker {
        image: artifact.image,
        digest,
        pull_command: format!("{authenticated_bootstrap} && docker compose pull"),
        apply_command: format!("{authenticated_bootstrap} && docker compose up -d"),
    })
}

fn parse_release_url(value: &str) -> Result<Url, ReleaseCheckErrorCode> {
    let url = Url::parse(value).map_err(|_| ReleaseCheckErrorCode::ManifestInvalid)?;
    let command_safe = url.as_str().bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(byte, b':' | b'/' | b'.' | b'_' | b'-' | b'~' | b'%')
    });
    if !is_safe_release_url(&url, false) || !command_safe {
        return Err(ReleaseCheckErrorCode::ManifestInvalid);
    }
    Ok(url)
}

fn validate_sha256(value: String) -> Result<String, ReleaseCheckErrorCode> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        Ok(value)
    } else {
        Err(ReleaseCheckErrorCode::ManifestInvalid)
    }
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer as _, SigningKey};

    use super::*;

    fn signed_envelope(mut manifest: serde_json::Value) -> (Vec<u8>, [u8; 32]) {
        let signing_key = SigningKey::from_bytes(&[42_u8; 32]);
        if manifest.get("schema_version").is_none() {
            manifest["schema_version"] = serde_json::json!(1);
        }
        let payload = serde_json::to_vec(&manifest).unwrap();
        let signature = signing_key.sign(&payload);
        let envelope = serde_json::json!({
            "payload": URL_SAFE_NO_PAD.encode(payload),
            "signature": URL_SAFE_NO_PAD.encode(signature.to_bytes()),
        });
        (
            serde_json::to_vec(&envelope).unwrap(),
            signing_key.verifying_key().to_bytes(),
        )
    }

    fn manifest() -> serde_json::Value {
        serde_json::json!({
            "schema_version": 1,
            "version": "1.0.1",
            "published_at": "2026-07-13T12:00:00Z",
            "notes_url": "https://github.com/thefrcrazy/DmxServerManager/releases/tag/v1.0.1",
            "native": {
                "linux_amd64": {
                    "archive_url": "https://github.com/thefrcrazy/DmxServerManager/releases/download/v1.0.1/dmx-server-manager-linux-amd64.tar.gz",
                    "archive_sha256": "a".repeat(64),
                    "installer_url": "https://github.com/thefrcrazy/DmxServerManager/releases/download/v1.0.1/dmx-server-manager-install-linux.sh",
                    "installer_sha256": "b".repeat(64)
                },
                "windows_amd64": {
                    "archive_url": "https://github.com/thefrcrazy/DmxServerManager/releases/download/v1.0.1/dmx-server-manager-windows-amd64.zip",
                    "archive_sha256": "c".repeat(64),
                    "installer_url": "https://github.com/thefrcrazy/DmxServerManager/releases/download/v1.0.1/dmx-server-manager-install-windows.ps1",
                    "installer_sha256": "d".repeat(64)
                }
            },
            "docker": {
                "image": RELEASE_IMAGE,
                "digest": format!("sha256:{}", "e".repeat(64))
            }
        })
    }

    #[test]
    fn verifies_signature_and_builds_only_fixed_native_command() {
        let (body, key) = signed_envelope(manifest());
        let release = verify_envelope(&body, key, DeploymentMode::Native).unwrap();
        assert_eq!(release.version, "1.0.1");
        let ReleaseTarget::Native {
            installer_sha256,
            upgrade_command,
            ..
        } = release.target
        else {
            panic!("expected native target");
        };
        assert_eq!(installer_sha256, "b".repeat(64));
        assert!(upgrade_command.contains("mktemp /tmp/dmx-server-manager-install.XXXXXX"));
        assert!(upgrade_command.contains("trap 'rm -f \"$p\"'"));
        assert!(upgrade_command.contains("sha256sum --check --status"));
        assert!(upgrade_command.contains("DMX_EXPECTED_ARCHIVE_SHA256='aaaaaaaa"));
        assert!(!upgrade_command.contains("latest"));
    }

    #[test]
    fn docker_commands_pin_the_signed_digest() {
        let (body, key) = signed_envelope(manifest());
        let release = verify_envelope(&body, key, DeploymentMode::Docker).unwrap();
        let ReleaseTarget::Docker {
            digest,
            pull_command,
            apply_command,
            ..
        } = release.target
        else {
            panic!("expected Docker target");
        };
        assert_eq!(digest, format!("sha256:{}", "e".repeat(64)));
        assert!(pull_command.contains(&digest));
        assert!(apply_command.contains(&digest));
        assert!(pull_command.contains("DMX_VERSION='1.0.1'"));
        assert!(pull_command.contains("./bootstrap-docker.sh direct"));
        assert!(pull_command.contains("--preserve-env=DMX_VERSION,DMX_IMAGE"));
        assert!(apply_command.contains("./bootstrap-docker.sh direct"));
        assert!(!pull_command.contains(":latest"));
    }

    #[test]
    fn windows_upgrade_command_pins_both_signed_checksums() {
        let target = validate_native_target(
            NativeArtifact {
                archive_url: "https://github.com/thefrcrazy/DmxServerManager/releases/download/v1.0.1/dmx-server-manager-windows-amd64.zip".into(),
                archive_sha256: "a".repeat(64),
                installer_url: "https://github.com/thefrcrazy/DmxServerManager/releases/download/v1.0.1/dmx-server-manager-install-windows.ps1".into(),
                installer_sha256: "b".repeat(64),
            },
            NativePlatform::WindowsAmd64,
            "1.0.1",
        )
        .unwrap();
        let ReleaseTarget::Native {
            upgrade_command, ..
        } = target
        else {
            panic!("expected native target");
        };
        assert!(upgrade_command.contains("-cne 'bbbbbbbb"));
        assert!(upgrade_command.contains("-ExpectedArchiveSha256 'aaaaaaaa"));
        assert!(upgrade_command.contains("[guid]::NewGuid()"));
        assert!(upgrade_command.contains("finally { Remove-Item -LiteralPath $p"));
        assert!(!upgrade_command.contains("latest"));
    }

    #[test]
    fn rejects_tampering_missing_checksums_and_unsafe_urls() {
        let (mut body, key) = signed_envelope(manifest());
        let last = body.len() - 2;
        body[last] ^= 1;
        assert!(matches!(
            verify_envelope(&body, key, DeploymentMode::Native),
            Err(ReleaseCheckErrorCode::EnvelopeInvalid | ReleaseCheckErrorCode::SignatureInvalid)
        ));

        let mut missing_checksum = manifest();
        missing_checksum["native"]["linux_amd64"]
            .as_object_mut()
            .unwrap()
            .remove("archive_sha256");
        let (body, key) = signed_envelope(missing_checksum);
        assert_eq!(
            verify_envelope(&body, key, DeploymentMode::Native),
            Err(ReleaseCheckErrorCode::ManifestInvalid)
        );

        let mut unsafe_url = manifest();
        unsafe_url["native"]["linux_amd64"]["installer_url"] =
            serde_json::json!("https://127.0.0.1/install.sh");
        let (body, key) = signed_envelope(unsafe_url);
        assert_eq!(
            verify_envelope(&body, key, DeploymentMode::Native),
            Err(ReleaseCheckErrorCode::ManifestInvalid)
        );
    }

    #[test]
    fn rejects_a_validly_signed_release_older_than_the_running_panel() {
        let (body, key) = signed_envelope(manifest());
        assert_eq!(
            verify_envelope_for_current(&body, key, DeploymentMode::Native, "1.0.2"),
            Err(ReleaseCheckErrorCode::ManifestInvalid)
        );
    }

    #[test]
    fn equal_release_is_up_to_date_and_failures_remove_previous_instructions() {
        let (body, key) = signed_envelope(manifest());
        let release =
            verify_envelope_for_current(&body, key, DeploymentMode::Native, "1.0.1").unwrap();
        let mut status = ReleaseStatus {
            configured: true,
            current_version: "1.0.1".into(),
            deployment_mode: DeploymentMode::Native,
            state: ReleaseCheckState::NeverChecked,
            checked_at: None,
            latest: None,
            error_code: None,
        };

        apply_check_result(&mut status, "2026-07-13T12:00:00Z".into(), Ok(release));
        assert_eq!(status.state, ReleaseCheckState::UpToDate);
        assert!(status.latest.is_some());

        apply_check_result(
            &mut status,
            "2026-07-13T13:00:00Z".into(),
            Err(ReleaseCheckErrorCode::Network),
        );
        assert_eq!(status.state, ReleaseCheckState::CheckFailed);
        assert_eq!(status.checked_at.as_deref(), Some("2026-07-13T13:00:00Z"));
        assert_eq!(status.latest, None);
        assert_eq!(status.error_code, Some(ReleaseCheckErrorCode::Network));
    }
}
