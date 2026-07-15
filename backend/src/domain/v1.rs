use std::{
    ffi::OsString,
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProfileKind {
    Builtin,
    SteamCustom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SupportedPlatform {
    #[serde(rename = "linux-x64")]
    LinuxX86_64,
    #[serde(rename = "windows-x64")]
    WindowsX86_64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum PortProtocol {
    Tcp,
    Udp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PortSpec {
    pub name: String,
    pub protocol: PortProtocol,
    pub default: u16,
    #[serde(default)]
    pub adjacent_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum StopStrategy {
    Stdin {
        command: String,
        timeout_seconds: u16,
    },
    Interrupt {
        timeout_seconds: u16,
    },
    Terminate {
        timeout_seconds: u16,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LifecycleSpec {
    pub stop: StopStrategy,
    #[serde(default)]
    pub ready_log_pattern: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GameProfile {
    pub id: String,
    pub revision: u32,
    pub name: String,
    pub description: String,
    pub kind: ProfileKind,
    pub platforms: Vec<SupportedPlatform>,
    pub capabilities: Vec<String>,
    pub ports: Vec<PortSpec>,
    pub lifecycle: LifecycleSpec,
    pub settings_schema: serde_json::Value,
    pub ui_schema: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steam_profile: Option<SteamProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstallationState {
    NotInstalled,
    Installing,
    Installed,
    Updating,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DesiredState {
    Running,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Crashed,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Instance {
    pub id: String,
    pub name: String,
    pub profile_id: String,
    pub profile_revision: u32,
    pub settings: serde_json::Value,
    pub config_version: u32,
    pub installation_state: InstallationState,
    pub installed_version: Option<String>,
    pub installed_build: Option<String>,
    pub desired_state: DesiredState,
    pub runtime_state: RuntimeState,
    pub managed: bool,
    pub auto_start: bool,
    pub watchdog_enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running,
    WaitingForUser,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind", deny_unknown_fields)]
pub enum JobInteraction {
    OauthDevice {
        verification_uri: String,
        user_code: Option<String>,
    },
    BedrockArchiveUpload {
        instance_id: String,
        version: Option<String>,
        method: String,
        path: String,
        required_sha256_header: String,
        max_bytes: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Job {
    pub id: String,
    pub instance_id: Option<String>,
    pub kind: String,
    pub state: JobState,
    pub progress: u8,
    pub requested_by: String,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub interaction: Option<JobInteraction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind", deny_unknown_fields)]
pub enum SteamStopStrategy {
    Stdin {
        command: String,
        timeout_seconds: u16,
    },
    Interrupt {
        timeout_seconds: u16,
    },
    Terminate {
        timeout_seconds: u16,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SteamExecutable {
    pub linux_x86_64: Option<String>,
    pub windows_x86_64: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SteamProfile {
    pub app_id: u32,
    pub branch: Option<String>,
    pub executable: SteamExecutable,
    #[serde(default)]
    pub arguments: Vec<String>,
    #[serde(default)]
    pub ports: Vec<PortSpec>,
    #[serde(default)]
    pub save_paths: Vec<String>,
    pub ready_log_pattern: Option<String>,
    pub stop_strategy: SteamStopStrategy,
}

/// A process specification whose paths and arguments have already been validated.
/// It deliberately carries an argument vector and never a shell command string.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct LaunchSpec {
    pub executable: PathBuf,
    pub cwd: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
}

#[allow(dead_code)]
impl LaunchSpec {
    pub fn for_instance(
        instance_root: &Path,
        executable: &str,
        cwd: &str,
        args: impl IntoIterator<Item = String>,
    ) -> Result<Self, String> {
        let executable = safe_join(instance_root, executable)?;
        let cwd = safe_join(instance_root, cwd)?;
        let args = args
            .into_iter()
            .map(|argument| {
                if argument.contains('\0') || argument.len() > 8_192 {
                    Err("invalid process argument".to_string())
                } else {
                    Ok(OsString::from(argument))
                }
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            executable,
            cwd,
            args,
            env: Vec::new(),
        })
    }
}

pub fn safe_join(root: &Path, relative: &str) -> Result<PathBuf, String> {
    if relative.is_empty()
        || relative.starts_with('/')
        || relative.starts_with('\\')
        || relative.as_bytes().get(1) == Some(&b':')
        || relative.contains('\0')
        || relative.chars().any(char::is_control)
    {
        return Err("path must be relative to the instance".to_string());
    }

    let normalized = relative.replace('\\', "/");
    if normalized.contains(':') {
        return Err("path contains a platform-specific stream or drive separator".to_string());
    }
    let path = Path::new(&normalized);
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err("path escapes the instance".to_string());
    }

    Ok(root.join(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_join_accepts_normal_instance_paths() {
        let root = Path::new("/data/instances/id");
        assert_eq!(
            safe_join(root, "Server/HytaleServer.jar").unwrap(),
            root.join("Server/HytaleServer.jar")
        );
    }

    #[test]
    fn safe_join_rejects_absolute_and_traversal_paths() {
        let root = Path::new("/data/instances/id");
        for path in [
            "../secret",
            "Server/../../secret",
            "/etc/passwd",
            "C:\\Windows\\System32\\cmd.exe",
            "\\\\server\\share",
            "server.exe:alternate-stream",
            "server\nname",
        ] {
            assert!(
                safe_join(root, path).is_err(),
                "accepted unsafe path {path}"
            );
        }
    }

    #[test]
    fn launch_spec_keeps_arguments_as_distinct_tokens() {
        let root = Path::new("/data/instances/id");
        let spec = LaunchSpec::for_instance(
            root,
            "server",
            ".",
            ["--name".to_string(), "value; rm -rf /".to_string()],
        )
        .unwrap();

        assert_eq!(spec.args.len(), 2);
        assert_eq!(spec.args[1], OsString::from("value; rm -rf /"));
    }
}
