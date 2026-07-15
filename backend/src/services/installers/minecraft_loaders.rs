//! Native installers for the Minecraft Java loaders that are not single-JAR
//! Mojang/Paper distributions.

use std::{
    collections::VecDeque,
    ffi::{OsStr, OsString},
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
    process::{ExitStatus, Stdio},
    time::Duration,
};

use reqwest::Url;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};

use crate::domain::v1::StopStrategy;

use super::{
    DownloadedFile, ExpectedDigest, InstallContext, InstallResult, InstalledArtifact,
    InstallerError, InstallerExecutable, InstallerPlan, VerifiedSource, download_verified,
    minecraft, read_bytes, read_json, toolchains,
};

const MAX_INSTALLER_BYTES: u64 = 128 * 1024 * 1024;
const MAX_SERVER_JAR_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ARGFILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_INSTALL_TREE_ENTRIES: usize = 250_000;
const OUTPUT_TAIL_BYTES: usize = 64 * 1024;

// Fabric Meta exposes the installer URL but not its digest. The official Maven
// repository does expose a SHA-256 sidecar. Keeping the exact release and digest
// here prevents a mutable "latest" installer from entering an instance.
const FABRIC_INSTALLER_VERSION: &str = "1.1.1";
const FABRIC_INSTALLER_SHA256: &str =
    "2487a69dd6f9d9c2605265a7142d77c26ab62edc620e6bcf810d581d2ee31b79";
const FABRIC_INSTALLER_SIZE: u64 = 209_151;

// Quilt Meta publishes size and SHA-256. This is still maintainer-pinned so a
// new installer cannot silently change an existing loader installation.
const QUILT_INSTALLER_VERSION: &str = "0.15.0";
const QUILT_INSTALLER_SHA256: &str =
    "f0c6e04e7f3b932d801b9e783ae17c960ff3cadc0f0109d6cc9be5240e99d455";
const QUILT_INSTALLER_SIZE: u64 = 7_381_964;

#[derive(Debug, Deserialize)]
struct FabricLoaderEntry {
    loader: LoaderCoordinate,
    intermediary: VersionCoordinate,
}

#[derive(Debug, Deserialize)]
struct QuiltLoaderEntry {
    loader: LoaderCoordinate,
    #[allow(dead_code)]
    hashed: Option<VersionCoordinate>,
}

#[derive(Debug, Deserialize)]
struct LoaderCoordinate {
    version: String,
}

#[derive(Debug, Deserialize)]
struct VersionCoordinate {
    version: String,
}

#[derive(Debug, Deserialize)]
struct PurpurVersionResponse {
    project: String,
    version: String,
    builds: PurpurBuilds,
}

#[derive(Debug, Deserialize)]
struct PurpurBuilds {
    all: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PurpurBuildResponse {
    project: String,
    version: String,
    build: String,
    result: String,
    md5: String,
}

#[derive(Debug, Deserialize)]
struct ModLoaderInstallProfile {
    minecraft: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DataKind {
    Mods,
    Plugins,
}

pub async fn install_fabric(
    settings: &Value,
    instance_root: &Path,
    staging: &Path,
    context: &InstallContext,
) -> Result<InstallResult, InstallerError> {
    minecraft::validate_eula(settings)?;
    let game_version = required_version(settings, "version")?;
    let loader_version = required_version(settings, "loader_version")?;
    let (_, java_major) = minecraft::imported_runtime(settings, staging, context).await?;
    validate_fabric_pair(context, &game_version, &loader_version).await?;
    let java = ensure_installer_java(context, java_major).await?;
    let source = fabric_installer_source(context)?;
    let installer = staging.join(".dmx-fabric-installer.jar");
    let downloaded = download_source(context, &source, &installer).await?;
    run_java_tool(
        &java,
        staging,
        &[
            "-jar",
            ".dmx-fabric-installer.jar",
            "server",
            "-mcversion",
            &game_version,
            "-loader",
            &loader_version,
            "-downloadMinecraft",
            "-dir",
            ".",
        ],
        Duration::from_secs(30 * 60),
        "fabric_installer",
    )
    .await?;
    remove_regular_if_exists(&installer).await?;
    finish_jar_loader_install(JarLoaderInstall {
        launcher_name: "fabric-server-launch.jar",
        game_version: &game_version,
        loader_version: &loader_version,
        java_major,
        settings,
        instance_root,
        staging,
        data_kind: DataKind::Mods,
        installer: downloaded,
    })
    .await
}

pub async fn install_quilt(
    settings: &Value,
    instance_root: &Path,
    staging: &Path,
    context: &InstallContext,
) -> Result<InstallResult, InstallerError> {
    minecraft::validate_eula(settings)?;
    let game_version = required_version(settings, "version")?;
    let loader_version = required_version(settings, "loader_version")?;
    let (_, java_major) = minecraft::imported_runtime(settings, staging, context).await?;
    validate_quilt_pair(context, &game_version, &loader_version).await?;
    let java = ensure_installer_java(context, java_major).await?;
    let source = quilt_installer_source(context)?;
    let installer = staging.join(".dmx-quilt-installer.jar");
    let downloaded = download_source(context, &source, &installer).await?;
    run_java_tool(
        &java,
        staging,
        &[
            "-jar",
            ".dmx-quilt-installer.jar",
            "install",
            "server",
            &game_version,
            &loader_version,
            "--download-server",
            "--install-dir=.",
        ],
        Duration::from_secs(30 * 60),
        "quilt_installer",
    )
    .await?;
    remove_regular_if_exists(&installer).await?;
    finish_jar_loader_install(JarLoaderInstall {
        launcher_name: "quilt-server-launch.jar",
        game_version: &game_version,
        loader_version: &loader_version,
        java_major,
        settings,
        instance_root,
        staging,
        data_kind: DataKind::Mods,
        installer: downloaded,
    })
    .await
}

pub async fn install_forge(
    settings: &Value,
    instance_root: &Path,
    staging: &Path,
    context: &InstallContext,
) -> Result<InstallResult, InstallerError> {
    install_mod_loader("forge", settings, instance_root, staging, context).await
}

pub async fn install_neoforge(
    settings: &Value,
    instance_root: &Path,
    staging: &Path,
    context: &InstallContext,
) -> Result<InstallResult, InstallerError> {
    install_mod_loader("neoforge", settings, instance_root, staging, context).await
}

pub async fn install_purpur(
    settings: &Value,
    instance_root: &Path,
    staging: &Path,
    context: &InstallContext,
) -> Result<InstallResult, InstallerError> {
    minecraft::validate_eula(settings)?;
    let game_version = required_version(settings, "version")?;
    let build = required_numeric_build(settings, "loader_version")?;
    let (_, java_major) = minecraft::imported_runtime(settings, staging, context).await?;
    let metadata = resolve_purpur_build(context, &game_version, &build).await?;
    validate_hex(&metadata.md5, 32)?;
    let url = provider_url(
        &context.sources.purpur_api_base,
        &format!("purpur/{game_version}/{build}/download"),
        "purpur_url_failed",
    )?;
    let downloaded = download_verified(
        context,
        &url,
        &staging.join("server.jar"),
        MAX_SERVER_JAR_BYTES,
        Some(&ExpectedDigest::Md5(metadata.md5)),
        None,
    )
    .await?;
    preserve_java_data(instance_root, staging, DataKind::Plugins).await?;
    minecraft::write_java_configuration(instance_root, staging, settings).await?;
    ensure_content_directory(staging, "plugins").await?;
    validate_install_tree(staging).await?;
    Ok(InstallResult {
        plan: jar_launch_plan(settings, java_major, "server.jar")?,
        installed_version: game_version,
        installed_build: Some(build),
        artifacts: vec![artifact_from_download("server.jar", downloaded)],
    })
}

pub async fn install_spigot(
    settings: &Value,
    instance_root: &Path,
    staging: &Path,
    context: &InstallContext,
) -> Result<InstallResult, InstallerError> {
    minecraft::validate_eula(settings)?;
    let game_version = required_version(settings, "version")?;
    let (_, java_major) = minecraft::imported_runtime(settings, staging, context).await?;
    let java = ensure_installer_java(context, java_major).await?;
    validate_source(&context.sources.buildtools)?;

    let work = staging.join(".dmx-buildtools-work");
    let output = staging.join(".dmx-buildtools-output");
    tokio::fs::create_dir(&work)
        .await
        .map_err(|error| InstallerError::internal("buildtools_staging_failed", error))?;
    tokio::fs::create_dir(&output)
        .await
        .map_err(|error| InstallerError::internal("buildtools_staging_failed", error))?;
    let installer = work.join("BuildTools.jar");
    download_source(context, &context.sources.buildtools, &installer).await?;
    run_java_tool(
        &java,
        &work,
        &[
            "-Xmx2G",
            "-jar",
            "BuildTools.jar",
            "--rev",
            &game_version,
            "--output-dir",
            "../.dmx-buildtools-output",
        ],
        Duration::from_secs(4 * 60 * 60),
        "buildtools",
    )
    .await?;
    let generated = output.join(format!("spigot-{game_version}.jar"));
    validate_regular_file(&generated, MAX_SERVER_JAR_BYTES).await?;
    tokio::fs::rename(&generated, staging.join("server.jar"))
        .await
        .map_err(|error| InstallerError::internal("buildtools_output_failed", error))?;
    remove_tree(&work).await?;
    remove_tree(&output).await?;
    preserve_java_data(instance_root, staging, DataKind::Plugins).await?;
    minecraft::write_java_configuration(instance_root, staging, settings).await?;
    ensure_content_directory(staging, "plugins").await?;
    validate_install_tree(staging).await?;
    let artifact = hash_artifact(&staging.join("server.jar"), "server.jar").await?;
    Ok(InstallResult {
        plan: jar_launch_plan(settings, java_major, "server.jar")?,
        installed_version: game_version,
        installed_build: Some(context.sources.buildtools.version.clone()),
        artifacts: vec![artifact],
    })
}

async fn install_mod_loader(
    loader: &'static str,
    settings: &Value,
    instance_root: &Path,
    staging: &Path,
    context: &InstallContext,
) -> Result<InstallResult, InstallerError> {
    minecraft::validate_eula(settings)?;
    let game_version = required_version(settings, "version")?;
    let loader_version = required_version(settings, "loader_version")?;
    if loader == "forge" && !loader_version.starts_with(&format!("{game_version}-")) {
        return Err(InstallerError::new(
            "forge_version_mismatch",
            "servers.minecraft_loader_version_mismatch",
        ));
    }
    if loader == "neoforge" {
        validate_neoforge_version(&game_version, &loader_version)?;
    }
    let (_, java_major) = minecraft::imported_runtime(settings, staging, context).await?;
    let source = mod_loader_installer_source(context, loader, &loader_version).await?;
    let java = ensure_installer_java(context, java_major).await?;
    let installer_name = format!(".dmx-{loader}-installer.jar");
    let installer = staging.join(&installer_name);
    let downloaded = download_source(context, &source, &installer).await?;
    validate_install_profile(&installer, &game_version).await?;
    run_java_tool(
        &java,
        staging,
        &["-jar", &installer_name, "--installServer"],
        Duration::from_secs(2 * 60 * 60),
        if loader == "forge" {
            "forge_installer"
        } else {
            "neoforge_installer"
        },
    )
    .await?;
    remove_regular_if_exists(&installer).await?;
    remove_generated_scripts(staging, loader, &loader_version).await?;
    let plan =
        mod_loader_launch_plan(loader, settings, java_major, &loader_version, staging).await?;
    preserve_java_data(instance_root, staging, DataKind::Mods).await?;
    minecraft::write_java_configuration(instance_root, staging, settings).await?;
    ensure_content_directory(staging, "mods").await?;
    validate_install_tree(staging).await?;
    Ok(InstallResult {
        plan,
        installed_version: game_version,
        installed_build: Some(loader_version),
        artifacts: vec![artifact_from_download(
            &format!("{loader}-installer.jar"),
            downloaded,
        )],
    })
}

struct JarLoaderInstall<'a> {
    launcher_name: &'static str,
    game_version: &'a str,
    loader_version: &'a str,
    java_major: u16,
    settings: &'a Value,
    instance_root: &'a Path,
    staging: &'a Path,
    data_kind: DataKind,
    installer: DownloadedFile,
}

async fn finish_jar_loader_install(
    input: JarLoaderInstall<'_>,
) -> Result<InstallResult, InstallerError> {
    validate_regular_file(
        &input.staging.join(input.launcher_name),
        MAX_SERVER_JAR_BYTES,
    )
    .await?;
    validate_regular_file(&input.staging.join("server.jar"), MAX_SERVER_JAR_BYTES).await?;
    preserve_java_data(input.instance_root, input.staging, input.data_kind).await?;
    minecraft::write_java_configuration(input.instance_root, input.staging, input.settings).await?;
    let content_directory = match input.data_kind {
        DataKind::Mods => "mods",
        DataKind::Plugins => "plugins",
    };
    ensure_content_directory(input.staging, content_directory).await?;
    validate_install_tree(input.staging).await?;
    let launcher = hash_artifact(
        &input.staging.join(input.launcher_name),
        input.launcher_name,
    )
    .await?;
    let server = hash_artifact(&input.staging.join("server.jar"), "server.jar").await?;
    Ok(InstallResult {
        plan: jar_launch_plan(input.settings, input.java_major, input.launcher_name)?,
        installed_version: input.game_version.to_string(),
        installed_build: Some(input.loader_version.to_string()),
        artifacts: vec![
            artifact_from_download("installer.jar", input.installer),
            launcher,
            server,
        ],
    })
}

pub fn launch_plan(
    profile_id: &str,
    settings: &Value,
    java_major: u16,
    installed_build: Option<&str>,
) -> Result<InstallerPlan, InstallerError> {
    match profile_id {
        "minecraft-java-fabric" => {
            let loader = installed_loader(settings, installed_build)?;
            let _ = loader;
            jar_launch_plan(settings, java_major, "fabric-server-launch.jar")
        }
        "minecraft-java-quilt" => {
            let loader = installed_loader(settings, installed_build)?;
            let _ = loader;
            jar_launch_plan(settings, java_major, "quilt-server-launch.jar")
        }
        "minecraft-java-purpur" | "minecraft-java-spigot" => {
            if profile_id == "minecraft-java-purpur" {
                let _ = installed_loader(settings, installed_build)?;
            }
            jar_launch_plan(settings, java_major, "server.jar")
        }
        "minecraft-java-forge" | "minecraft-java-neoforge" => {
            let loader = installed_loader(settings, installed_build)?;
            mod_loader_plan_from_relative(profile_id, settings, java_major, loader)
        }
        _ => Err(InstallerError::new(
            "runtime_not_implemented",
            "servers.runtime_not_implemented",
        )),
    }
}

pub async fn validate_installed(
    profile_id: &str,
    settings: &Value,
    game_root: &Path,
    java_major: u16,
    installed_build: Option<&str>,
) -> Result<InstallerPlan, InstallerError> {
    minecraft::validate_eula(settings)?;
    let plan = launch_plan(profile_id, settings, java_major, installed_build)?;
    match profile_id {
        "minecraft-java-fabric" => {
            validate_regular_file(
                &game_root.join("fabric-server-launch.jar"),
                MAX_SERVER_JAR_BYTES,
            )
            .await?;
            validate_regular_file(&game_root.join("server.jar"), MAX_SERVER_JAR_BYTES).await?;
        }
        "minecraft-java-quilt" => {
            validate_regular_file(
                &game_root.join("quilt-server-launch.jar"),
                MAX_SERVER_JAR_BYTES,
            )
            .await?;
            validate_regular_file(&game_root.join("server.jar"), MAX_SERVER_JAR_BYTES).await?;
        }
        "minecraft-java-purpur" | "minecraft-java-spigot" => {
            validate_regular_file(&game_root.join("server.jar"), MAX_SERVER_JAR_BYTES).await?;
        }
        "minecraft-java-forge" | "minecraft-java-neoforge" => {
            validate_argfile_for_plan(game_root, &plan).await?;
        }
        _ => {
            return Err(InstallerError::new(
                "runtime_not_implemented",
                "servers.runtime_not_implemented",
            ));
        }
    }
    validate_install_tree(game_root).await?;
    Ok(plan)
}

pub async fn validate_import(
    profile_id: &str,
    settings: &Value,
    game_root: &Path,
    java_major: u16,
    context: &InstallContext,
) -> Result<InstallerPlan, InstallerError> {
    let game_version = required_version(settings, "version")?;
    let loader = match profile_id {
        "minecraft-java-fabric"
        | "minecraft-java-forge"
        | "minecraft-java-neoforge"
        | "minecraft-java-purpur"
        | "minecraft-java-quilt" => Some(required_version(settings, "loader_version")?),
        "minecraft-java-spigot" => None,
        _ => {
            return Err(InstallerError::new(
                "import_profile_unsupported",
                "imports.profile_not_supported",
            ));
        }
    };
    match profile_id {
        "minecraft-java-fabric" => {
            validate_fabric_pair(
                context,
                &game_version,
                loader.as_deref().unwrap_or_default(),
            )
            .await?;
        }
        "minecraft-java-quilt" => {
            validate_quilt_pair(
                context,
                &game_version,
                loader.as_deref().unwrap_or_default(),
            )
            .await?;
        }
        "minecraft-java-purpur" => {
            resolve_purpur_build(
                context,
                &game_version,
                loader.as_deref().unwrap_or_default(),
            )
            .await?;
        }
        "minecraft-java-forge" => {
            if !loader
                .as_deref()
                .is_some_and(|value| value.starts_with(&format!("{game_version}-")))
            {
                return Err(InstallerError::new(
                    "forge_version_mismatch",
                    "servers.minecraft_loader_version_mismatch",
                ));
            }
        }
        "minecraft-java-neoforge" => {
            validate_neoforge_version(&game_version, loader.as_deref().unwrap_or_default())?;
        }
        _ => {}
    }
    let plan = match profile_id {
        "minecraft-java-fabric" => {
            validate_regular_file(
                &game_root.join("fabric-server-launch.jar"),
                MAX_SERVER_JAR_BYTES,
            )
            .await?;
            jar_launch_plan(settings, java_major, "fabric-server-launch.jar")?
        }
        "minecraft-java-quilt" => {
            validate_regular_file(
                &game_root.join("quilt-server-launch.jar"),
                MAX_SERVER_JAR_BYTES,
            )
            .await?;
            jar_launch_plan(settings, java_major, "quilt-server-launch.jar")?
        }
        "minecraft-java-purpur" | "minecraft-java-spigot" => {
            validate_regular_file(&game_root.join("server.jar"), MAX_SERVER_JAR_BYTES).await?;
            jar_launch_plan(settings, java_major, "server.jar")?
        }
        "minecraft-java-forge" | "minecraft-java-neoforge" => {
            let loader = loader.as_deref().ok_or_else(|| {
                InstallerError::new("settings_invalid", "servers.settings_invalid")
            })?;
            let plan = mod_loader_plan_from_relative(profile_id, settings, java_major, loader)?;
            validate_argfile_for_plan(game_root, &plan).await?;
            plan
        }
        _ => unreachable!(),
    };
    validate_install_tree(game_root).await?;
    Ok(plan)
}

fn installed_loader<'a>(
    settings: &Value,
    installed_build: Option<&'a str>,
) -> Result<&'a str, InstallerError> {
    let requested = required_version(settings, "loader_version")?;
    let installed = installed_build.ok_or_else(|| {
        InstallerError::new("loader_version_missing", "servers.install_metadata_invalid")
    })?;
    if requested != installed {
        return Err(InstallerError::new(
            "loader_version_changed",
            "servers.minecraft_loader_reinstall_required",
        ));
    }
    Ok(installed)
}

fn jar_launch_plan(
    settings: &Value,
    java_major: u16,
    jar_name: &'static str,
) -> Result<InstallerPlan, InstallerError> {
    let mut plan = minecraft::launch_plan(settings, java_major)?;
    let jar = plan.args.get_mut(2).ok_or_else(|| {
        InstallerError::new("launch_plan_invalid", "servers.runtime_not_implemented")
    })?;
    *jar = jar_name.to_string();
    Ok(plan)
}

async fn mod_loader_launch_plan(
    loader: &str,
    settings: &Value,
    java_major: u16,
    loader_version: &str,
    staging: &Path,
) -> Result<InstallerPlan, InstallerError> {
    let profile_id = if loader == "forge" {
        "minecraft-java-forge"
    } else {
        "minecraft-java-neoforge"
    };
    let plan = mod_loader_plan_from_relative(profile_id, settings, java_major, loader_version)?;
    if validate_argfile_for_plan(staging, &plan).await.is_ok() {
        return Ok(plan);
    }
    if loader == "forge" {
        let legacy = format!("forge-{loader_version}.jar");
        validate_regular_file(&staging.join(&legacy), MAX_SERVER_JAR_BYTES).await?;
        return jar_launch_plan_dynamic(settings, java_major, legacy);
    }
    Err(InstallerError::new(
        "loader_runtime_layout_invalid",
        "servers.minecraft_loader_runtime_invalid",
    ))
}

fn mod_loader_plan_from_relative(
    profile_id: &str,
    settings: &Value,
    java_major: u16,
    loader_version: &str,
) -> Result<InstallerPlan, InstallerError> {
    validate_version_value(loader_version)?;
    let coordinate = match profile_id {
        "minecraft-java-forge" => "net/minecraftforge/forge",
        "minecraft-java-neoforge" => "net/neoforged/neoforge",
        _ => {
            return Err(InstallerError::new(
                "runtime_not_implemented",
                "servers.runtime_not_implemented",
            ));
        }
    };
    let platform_argfile = if cfg!(windows) {
        "win_args.txt"
    } else {
        "unix_args.txt"
    };
    let relative = format!("libraries/{coordinate}/{loader_version}/{platform_argfile}");
    let memory = memory_argument(settings)?;
    Ok(InstallerPlan {
        executable: InstallerExecutable::ManagedJava { major: java_major },
        cwd_relative: ".".to_string(),
        args: vec![memory, format!("@{relative}"), "nogui".to_string()],
        env: Vec::new(),
        stop: StopStrategy::Stdin {
            command: "stop".to_string(),
            timeout_seconds: 60,
        },
        restart_exit_codes: Vec::new(),
    })
}

fn jar_launch_plan_dynamic(
    settings: &Value,
    java_major: u16,
    jar_name: String,
) -> Result<InstallerPlan, InstallerError> {
    validate_relative_component(&jar_name)?;
    let mut plan = minecraft::launch_plan(settings, java_major)?;
    plan.args[2] = jar_name;
    Ok(plan)
}

fn memory_argument(settings: &Value) -> Result<String, InstallerError> {
    let memory = settings
        .get("max_memory_mb")
        .and_then(Value::as_u64)
        .unwrap_or(4096);
    if !(512..=131_072).contains(&memory) {
        return Err(InstallerError::new(
            "minecraft_memory_invalid",
            "servers.settings_invalid",
        ));
    }
    Ok(format!("-Xmx{memory}M"))
}

async fn validate_fabric_pair(
    context: &InstallContext,
    game_version: &str,
    loader_version: &str,
) -> Result<(), InstallerError> {
    let url = provider_url(
        &context.sources.fabric_meta_base,
        &format!("versions/loader/{game_version}/{loader_version}"),
        "fabric_url_failed",
    )?;
    let entry: FabricLoaderEntry = read_json(context, &url).await?;
    if entry.loader.version == loader_version && entry.intermediary.version == game_version {
        Ok(())
    } else {
        Err(InstallerError::new(
            "fabric_version_unavailable",
            "servers.minecraft_loader_version_unavailable",
        ))
    }
}

async fn validate_quilt_pair(
    context: &InstallContext,
    game_version: &str,
    loader_version: &str,
) -> Result<(), InstallerError> {
    let url = provider_url(
        &context.sources.quilt_meta_base,
        &format!("versions/loader/{game_version}"),
        "quilt_url_failed",
    )?;
    let entries: Vec<QuiltLoaderEntry> = read_json(context, &url).await?;
    if entries
        .iter()
        .any(|entry| entry.loader.version == loader_version)
    {
        Ok(())
    } else {
        Err(InstallerError::new(
            "quilt_version_unavailable",
            "servers.minecraft_loader_version_unavailable",
        ))
    }
}

async fn resolve_purpur_build(
    context: &InstallContext,
    game_version: &str,
    build: &str,
) -> Result<PurpurBuildResponse, InstallerError> {
    let version_url = provider_url(
        &context.sources.purpur_api_base,
        &format!("purpur/{game_version}"),
        "purpur_url_failed",
    )?;
    let version: PurpurVersionResponse = read_json(context, &version_url).await?;
    if version.project != "purpur"
        || version.version != game_version
        || !version.builds.all.iter().any(|value| value == build)
    {
        return Err(InstallerError::new(
            "purpur_build_unavailable",
            "servers.minecraft_loader_version_unavailable",
        ));
    }
    let build_url = provider_url(
        &context.sources.purpur_api_base,
        &format!("purpur/{game_version}/{build}"),
        "purpur_url_failed",
    )?;
    let metadata: PurpurBuildResponse = read_json(context, &build_url).await?;
    if metadata.project != "purpur"
        || metadata.version != game_version
        || metadata.build != build
        || metadata.result != "SUCCESS"
    {
        return Err(InstallerError::new(
            "purpur_build_invalid",
            "servers.provider_response_invalid",
        ));
    }
    Ok(metadata)
}

fn fabric_installer_source(context: &InstallContext) -> Result<VerifiedSource, InstallerError> {
    Ok(VerifiedSource {
        url: provider_url(
            &context.sources.fabric_maven_base,
            &format!(
                "net/fabricmc/fabric-installer/{0}/fabric-installer-{0}.jar",
                FABRIC_INSTALLER_VERSION
            ),
            "fabric_url_failed",
        )?,
        sha256: FABRIC_INSTALLER_SHA256.to_string(),
        size: Some(FABRIC_INSTALLER_SIZE),
        version: FABRIC_INSTALLER_VERSION.to_string(),
    })
}

fn quilt_installer_source(context: &InstallContext) -> Result<VerifiedSource, InstallerError> {
    Ok(VerifiedSource {
        url: provider_url(
            &context.sources.quilt_maven_base,
            &format!(
                "org/quiltmc/quilt-installer/{0}/quilt-installer-{0}.jar",
                QUILT_INSTALLER_VERSION
            ),
            "quilt_url_failed",
        )?,
        sha256: QUILT_INSTALLER_SHA256.to_string(),
        size: Some(QUILT_INSTALLER_SIZE),
        version: QUILT_INSTALLER_VERSION.to_string(),
    })
}

async fn mod_loader_installer_source(
    context: &InstallContext,
    loader: &str,
    loader_version: &str,
) -> Result<VerifiedSource, InstallerError> {
    validate_version_value(loader_version)?;
    let (base, relative) = match loader {
        "forge" => (
            &context.sources.forge_maven_base,
            format!(
                "net/minecraftforge/forge/{0}/forge-{0}-installer.jar",
                loader_version
            ),
        ),
        "neoforge" => (
            &context.sources.neoforge_maven_base,
            format!(
                "net/neoforged/neoforge/{0}/neoforge-{0}-installer.jar",
                loader_version
            ),
        ),
        _ => {
            return Err(InstallerError::new(
                "loader_unknown",
                "servers.settings_invalid",
            ));
        }
    };
    let url = provider_url(base, &relative, "loader_url_failed")?;
    let sha_url = Url::parse(&format!("{}.sha256", url.as_str()))
        .map_err(|error| InstallerError::internal("loader_checksum_url_failed", error))?;
    let checksum = read_checksum(context, &sha_url, 64).await?;
    Ok(VerifiedSource {
        url,
        sha256: checksum,
        size: None,
        version: loader_version.to_string(),
    })
}

async fn read_checksum(
    context: &InstallContext,
    url: &Url,
    length: usize,
) -> Result<String, InstallerError> {
    let bytes = read_bytes(context, url, 1024).await?;
    let text = std::str::from_utf8(&bytes).map_err(|_| {
        InstallerError::new(
            "provider_checksum_invalid",
            "servers.provider_response_invalid",
        )
    })?;
    let checksum = text.split_ascii_whitespace().next().unwrap_or_default();
    validate_hex(checksum, length)?;
    Ok(checksum.to_ascii_lowercase())
}

async fn download_source(
    context: &InstallContext,
    source: &VerifiedSource,
    destination: &Path,
) -> Result<DownloadedFile, InstallerError> {
    validate_source(source)?;
    download_verified(
        context,
        &source.url,
        destination,
        MAX_INSTALLER_BYTES,
        Some(&ExpectedDigest::Sha256(source.sha256.clone())),
        source.size,
    )
    .await
}

fn validate_source(source: &VerifiedSource) -> Result<(), InstallerError> {
    validate_hex(&source.sha256, 64)?;
    if source.version.is_empty() || source.version.len() > 96 {
        return Err(InstallerError::new(
            "provider_version_invalid",
            "servers.provider_response_invalid",
        ));
    }
    if source.size == Some(0) || source.size.is_some_and(|size| size > MAX_INSTALLER_BYTES) {
        return Err(InstallerError::new(
            "provider_size_invalid",
            "servers.provider_response_invalid",
        ));
    }
    Ok(())
}

fn provider_url(base: &Url, relative: &str, code: &'static str) -> Result<Url, InstallerError> {
    base.join(relative)
        .map_err(|error| InstallerError::internal(code, error))
}

async fn ensure_installer_java(
    context: &InstallContext,
    java_major: u16,
) -> Result<PathBuf, InstallerError> {
    let root = context.toolchain_root.as_ref().ok_or_else(|| {
        InstallerError::new(
            "java_toolchain_root_missing",
            "servers.java_runtime_unavailable",
        )
    })?;
    toolchains::ensure_java(root, java_major, context).await
}

async fn validate_install_profile(
    installer: &Path,
    expected_game_version: &str,
) -> Result<(), InstallerError> {
    let installer = installer.to_path_buf();
    let expected = expected_game_version.to_string();
    tokio::task::spawn_blocking(move || {
        let file = File::open(&installer)
            .map_err(|error| InstallerError::internal("loader_installer_invalid", error))?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|error| InstallerError::internal("loader_installer_invalid", error))?;
        let profile = archive.by_name("install_profile.json").map_err(|_| {
            InstallerError::new(
                "loader_install_profile_missing",
                "servers.provider_response_invalid",
            )
        })?;
        if profile.size() > 4 * 1024 * 1024 || profile.is_dir() {
            return Err(InstallerError::new(
                "loader_install_profile_invalid",
                "servers.provider_response_invalid",
            ));
        }
        let mut bytes = Vec::with_capacity(profile.size() as usize);
        profile
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| InstallerError::internal("loader_install_profile_invalid", error))?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(InstallerError::new(
                "loader_install_profile_invalid",
                "servers.provider_response_invalid",
            ));
        }
        let profile: ModLoaderInstallProfile = serde_json::from_slice(&bytes)
            .map_err(|error| InstallerError::internal("loader_install_profile_invalid", error))?;
        if profile.minecraft != expected {
            return Err(InstallerError::new(
                "loader_game_version_mismatch",
                "servers.minecraft_loader_version_mismatch",
            ));
        }
        Ok(())
    })
    .await
    .map_err(|error| InstallerError::internal("loader_profile_worker_failed", error))?
}

async fn preserve_java_data(
    instance_root: &Path,
    staging: &Path,
    kind: DataKind,
) -> Result<(), InstallerError> {
    let current = instance_root.join("game");
    let mut paths = minecraft::preserved_world_paths(instance_root).await?;
    paths.extend(
        [
            "ops.json",
            "whitelist.json",
            "banned-players.json",
            "banned-ips.json",
        ]
        .map(str::to_string),
    );
    match kind {
        DataKind::Mods => paths.extend(["mods", "config", "defaultconfigs"].map(str::to_string)),
        DataKind::Plugins => paths.extend(
            [
                "plugins",
                "config",
                "bukkit.yml",
                "spigot.yml",
                "paper.yml",
                "paper-global.yml",
                "paper-world-defaults.yml",
                "purpur.yml",
            ]
            .map(str::to_string),
        ),
    }
    for relative in paths {
        minecraft::copy_optional_without_links(&current.join(&relative), &staging.join(&relative))
            .await?;
    }
    Ok(())
}

async fn ensure_content_directory(root: &Path, relative: &str) -> Result<(), InstallerError> {
    let path = root.join(relative);
    match tokio::fs::symlink_metadata(&path).await {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(InstallerError::new(
            "content_directory_invalid",
            "servers.instance_data_unsafe",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => tokio::fs::create_dir(&path)
            .await
            .map_err(|error| InstallerError::internal("content_directory_failed", error)),
        Err(error) => Err(InstallerError::internal("content_directory_failed", error)),
    }
}

async fn remove_generated_scripts(
    staging: &Path,
    loader: &str,
    loader_version: &str,
) -> Result<(), InstallerError> {
    for relative in [
        "run.sh",
        "run.bat",
        "user_jvm_args.txt",
        &format!("{loader}-{loader_version}-installer.jar.log"),
    ] {
        remove_regular_if_exists(&staging.join(relative)).await?;
    }
    Ok(())
}

async fn remove_regular_if_exists(path: &Path) -> Result<(), InstallerError> {
    match tokio::fs::symlink_metadata(path).await {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(InstallerError::internal("installer_cleanup_failed", error)),
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            tokio::fs::remove_file(path)
                .await
                .map_err(|error| InstallerError::internal("installer_cleanup_failed", error))
        }
        Ok(_) => Err(InstallerError::new(
            "installer_cleanup_unsafe",
            "servers.instance_data_unsafe",
        )),
    }
}

async fn remove_tree(path: &Path) -> Result<(), InstallerError> {
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|error| InstallerError::internal("installer_cleanup_failed", error))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(InstallerError::new(
            "installer_cleanup_unsafe",
            "servers.instance_data_unsafe",
        ));
    }
    tokio::fs::remove_dir_all(path)
        .await
        .map_err(|error| InstallerError::internal("installer_cleanup_failed", error))
}

async fn validate_regular_file(path: &Path, max_bytes: u64) -> Result<u64, InstallerError> {
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|error| InstallerError::internal("loader_runtime_missing", error))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > max_bytes
        || link_count_is_unsafe(path, &metadata)?
    {
        return Err(InstallerError::new(
            "loader_runtime_invalid",
            "servers.minecraft_loader_runtime_invalid",
        ));
    }
    Ok(metadata.len())
}

async fn validate_install_tree(root: &Path) -> Result<(), InstallerError> {
    let root_metadata = tokio::fs::symlink_metadata(root)
        .await
        .map_err(|error| InstallerError::internal("install_tree_invalid", error))?;
    if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
        return Err(InstallerError::new(
            "install_tree_unsafe",
            "servers.instance_data_unsafe",
        ));
    }
    let mut pending = vec![root.to_path_buf()];
    let mut count = 0_usize;
    while let Some(directory) = pending.pop() {
        let mut entries = tokio::fs::read_dir(&directory)
            .await
            .map_err(|error| InstallerError::internal("install_tree_invalid", error))?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|error| InstallerError::internal("install_tree_invalid", error))?
        {
            count = count.checked_add(1).ok_or_else(|| {
                InstallerError::new("install_tree_too_large", "servers.installation_failed")
            })?;
            if count > MAX_INSTALL_TREE_ENTRIES {
                return Err(InstallerError::new(
                    "install_tree_too_large",
                    "servers.installation_failed",
                ));
            }
            let metadata = entry
                .metadata()
                .await
                .map_err(|error| InstallerError::internal("install_tree_invalid", error))?;
            let file_type = entry
                .file_type()
                .await
                .map_err(|error| InstallerError::internal("install_tree_invalid", error))?;
            let entry_path = entry.path();
            if file_type.is_symlink()
                || (!file_type.is_file() && !file_type.is_dir())
                || (file_type.is_file() && link_count_is_unsafe(&entry_path, &metadata)?)
            {
                return Err(InstallerError::new(
                    "install_tree_unsafe",
                    "servers.instance_data_unsafe",
                ));
            }
            if file_type.is_dir() {
                pending.push(entry_path);
            }
        }
    }
    Ok(())
}

async fn hash_artifact(path: &Path, name: &str) -> Result<InstalledArtifact, InstallerError> {
    let size = validate_regular_file(path, MAX_SERVER_JAR_BYTES).await?;
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|error| InstallerError::internal("artifact_hash_failed", error))?;
    let mut digest = Sha256::new();
    let mut read = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .await
            .map_err(|error| InstallerError::internal("artifact_hash_failed", error))?;
        if count == 0 {
            break;
        }
        read = read.checked_add(count as u64).ok_or_else(|| {
            InstallerError::new("artifact_too_large", "servers.artifact_integrity_failed")
        })?;
        if read > MAX_SERVER_JAR_BYTES {
            return Err(InstallerError::new(
                "artifact_too_large",
                "servers.artifact_integrity_failed",
            ));
        }
        digest.update(&buffer[..count]);
    }
    if read != size {
        return Err(InstallerError::new(
            "artifact_changed",
            "servers.artifact_integrity_failed",
        ));
    }
    Ok(InstalledArtifact {
        name: name.to_string(),
        sha256: format!("{:x}", digest.finalize()),
        size,
    })
}

fn artifact_from_download(name: &str, downloaded: DownloadedFile) -> InstalledArtifact {
    InstalledArtifact {
        name: name.to_string(),
        sha256: downloaded.sha256,
        size: downloaded.size,
    }
}

async fn validate_argfile_for_plan(
    root: &Path,
    plan: &InstallerPlan,
) -> Result<(), InstallerError> {
    let argument = plan
        .args
        .iter()
        .find_map(|argument| argument.strip_prefix('@'))
        .ok_or_else(|| {
            InstallerError::new(
                "loader_argfile_missing",
                "servers.minecraft_loader_runtime_invalid",
            )
        })?;
    validate_relative_path(argument)?;
    let path = root.join(argument);
    validate_regular_file(&path, MAX_ARGFILE_BYTES).await?;
    let contents = tokio::fs::read_to_string(&path)
        .await
        .map_err(|error| InstallerError::internal("loader_argfile_invalid", error))?;
    validate_argfile_contents(&contents)
}

fn validate_argfile_contents(contents: &str) -> Result<(), InstallerError> {
    if contents.is_empty() || contents.len() as u64 > MAX_ARGFILE_BYTES || contents.contains('\0') {
        return Err(InstallerError::new(
            "loader_argfile_invalid",
            "servers.minecraft_loader_runtime_invalid",
        ));
    }
    // Java argfiles are intentionally interpreted by the JVM. Reject nested
    // argfiles, host-absolute paths and agent/error-handler options. The
    // remaining file was generated inside staging by the checksummed official
    // installer and is never accepted from an API value.
    for token in contents.split_ascii_whitespace() {
        let unquoted = token.trim_matches(['\'', '"']);
        if unquoted.starts_with('@')
            || contains_host_absolute_path(unquoted)
            || unquoted.contains("../")
            || unquoted.contains("..\\")
            || unquoted.starts_with("-javaagent")
            || unquoted.starts_with("-agentlib")
            || unquoted.starts_with("-agentpath")
            || unquoted.starts_with("-XX:OnError")
            || unquoted.starts_with("-XX:OnOutOfMemoryError")
        {
            return Err(InstallerError::new(
                "loader_argfile_unsafe",
                "servers.minecraft_loader_runtime_invalid",
            ));
        }
    }
    Ok(())
}

fn contains_host_absolute_path(token: &str) -> bool {
    let looks_absolute = |candidate: &str| {
        let bytes = candidate.as_bytes();
        Path::new(candidate).is_absolute()
            || candidate.starts_with('/')
            || candidate.starts_with('\\')
            || (bytes.len() >= 3
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && matches!(bytes[2], b'/' | b'\\'))
    };
    if looks_absolute(token) {
        return true;
    }
    let value = token.split_once('=').map_or(token, |(_, value)| value);
    looks_absolute(value)
        || value
            .split([';', ':'])
            .any(|component| looks_absolute(component.trim_matches(['\'', '"'])))
}

fn required_version(settings: &Value, key: &'static str) -> Result<String, InstallerError> {
    let value = settings
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| InstallerError::new("settings_invalid", "servers.settings_invalid"))?;
    validate_version_value(value)?;
    Ok(value.to_string())
}

fn validate_neoforge_version(
    game_version: &str,
    loader_version: &str,
) -> Result<(), InstallerError> {
    let expected_line = if let Some(legacy) = game_version.strip_prefix("1.") {
        let mut components = legacy.split('.');
        let minor = components.next().unwrap_or_default();
        let patch = components.next();
        if minor.is_empty()
            || !minor.bytes().all(|byte| byte.is_ascii_digit())
            || patch.is_some_and(|value| {
                value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit())
            })
            || components.next().is_some()
        {
            return Err(InstallerError::new(
                "minecraft_version_unsupported",
                "servers.minecraft_loader_version_mismatch",
            ));
        }
        format!("{minor}.{}", patch.unwrap_or("0"))
    } else {
        game_version.to_string()
    };
    if loader_version == expected_line
        || loader_version
            .strip_prefix(&expected_line)
            .is_some_and(|rest| rest.starts_with('.'))
    {
        Ok(())
    } else {
        Err(InstallerError::new(
            "neoforge_version_mismatch",
            "servers.minecraft_loader_version_mismatch",
        ))
    }
}

fn required_numeric_build(settings: &Value, key: &'static str) -> Result<String, InstallerError> {
    let value = required_version(settings, key)?;
    if value.len() > 12 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(InstallerError::new(
            "loader_build_invalid",
            "servers.settings_invalid",
        ));
    }
    let parsed = value
        .parse::<u64>()
        .map_err(|_| InstallerError::new("loader_build_invalid", "servers.settings_invalid"))?;
    if parsed == 0 || parsed.to_string() != value {
        return Err(InstallerError::new(
            "loader_build_invalid",
            "servers.settings_invalid",
        ));
    }
    Ok(value)
}

fn validate_version_value(value: &str) -> Result<(), InstallerError> {
    if value.is_empty()
        || value.len() > 96
        || matches!(
            value.to_ascii_lowercase().as_str(),
            "latest" | "recommended" | "stable"
        )
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+'))
    {
        return Err(InstallerError::new(
            "version_identifier_invalid",
            "servers.settings_invalid",
        ));
    }
    Ok(())
}

fn validate_relative_component(value: &str) -> Result<(), InstallerError> {
    if value.is_empty()
        || value.len() > 255
        || Path::new(value).file_name().and_then(OsStr::to_str) != Some(value)
        || value
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control())
    {
        return Err(InstallerError::new(
            "runtime_path_invalid",
            "servers.minecraft_loader_runtime_invalid",
        ));
    }
    Ok(())
}

fn validate_relative_path(value: &str) -> Result<(), InstallerError> {
    if value.is_empty()
        || value.len() > 2048
        || Path::new(value).is_absolute()
        || Path::new(value).components().any(|component| {
            !matches!(component, Component::Normal(_))
                || component
                    .as_os_str()
                    .to_str()
                    .is_none_or(|part| part.contains(':') || part.contains('\0'))
        })
    {
        return Err(InstallerError::new(
            "runtime_path_invalid",
            "servers.minecraft_loader_runtime_invalid",
        ));
    }
    Ok(())
}

fn validate_hex(value: &str, length: usize) -> Result<(), InstallerError> {
    if value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(InstallerError::new(
            "provider_checksum_invalid",
            "servers.provider_response_invalid",
        ))
    }
}

fn link_count_is_unsafe(path: &Path, metadata: &std::fs::Metadata) -> Result<bool, InstallerError> {
    crate::services::secure_fs::file_has_multiple_links(path, metadata)
        .map_err(|error| InstallerError::internal("install_tree_invalid", error))
}

struct ToolOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

async fn run_java_tool(
    java: &Path,
    cwd: &Path,
    args: &[&str],
    timeout: Duration,
    code: &'static str,
) -> Result<(), InstallerError> {
    let mut command = Command::new(java);
    command
        .current_dir(cwd)
        .args(args)
        .env_clear()
        .envs(filtered_tool_environment())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    configure_process_group(&mut command)
        .map_err(|error| InstallerError::internal("installer_containment_failed", error))?;
    let mut child = command
        .spawn()
        .map_err(|error| InstallerError::internal("installer_start_failed", error))?;
    let mut group = ProcessGroupGuard::new(&mut child)?;
    let stdout = child.stdout.take().ok_or_else(|| {
        InstallerError::new("installer_output_missing", "servers.installation_failed")
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        InstallerError::new("installer_output_missing", "servers.installation_failed")
    })?;
    let stdout_task = tokio::spawn(read_tail(stdout));
    let stderr_task = tokio::spawn(read_tail(stderr));
    let status = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(result) => {
            result.map_err(|error| InstallerError::internal("installer_wait_failed", error))?
        }
        Err(_) => {
            group.kill();
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(InstallerError::new(
                "installer_process_timeout",
                "servers.installation_timeout",
            ));
        }
    };
    group.finish();
    let stdout = stdout_task
        .await
        .map_err(|error| InstallerError::internal("installer_output_failed", error))??;
    let stderr = stderr_task
        .await
        .map_err(|error| InstallerError::internal("installer_output_failed", error))??;
    let output = ToolOutput {
        status,
        stdout,
        stderr,
    };
    if !output.status.success() {
        let detail = format_tool_failure(code, &output);
        return Err(InstallerError {
            code,
            client_message: "servers.minecraft_loader_install_failed",
            internal: Some(detail),
        });
    }
    Ok(())
}

async fn read_tail<R>(mut reader: R) -> Result<Vec<u8>, InstallerError>
where
    R: AsyncRead + Unpin,
{
    let mut tail = VecDeque::with_capacity(OUTPUT_TAIL_BYTES);
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .await
            .map_err(|error| InstallerError::internal("installer_output_failed", error))?;
        if read == 0 {
            break;
        }
        for byte in &buffer[..read] {
            if tail.len() == OUTPUT_TAIL_BYTES {
                tail.pop_front();
            }
            tail.push_back(*byte);
        }
    }
    Ok(tail.into_iter().collect())
}

fn format_tool_failure(code: &str, output: &ToolOutput) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let sanitize = |value: &str| {
        value
            .chars()
            .map(|character| {
                if character.is_control() && !matches!(character, '\n' | '\r' | '\t') {
                    '\u{fffd}'
                } else {
                    character
                }
            })
            .collect::<String>()
    };
    format!(
        "{code} exited with {:?}; stdout tail: {}; stderr tail: {}",
        output.status.code(),
        sanitize(&stdout),
        sanitize(&stderr)
    )
}

fn filtered_tool_environment() -> Vec<(OsString, OsString)> {
    [
        "PATH",
        "HOME",
        "USERPROFILE",
        "SYSTEMROOT",
        "TMP",
        "TEMP",
        "TMPDIR",
    ]
    .into_iter()
    .filter_map(|key| std::env::var_os(key).map(|value| (OsString::from(key), value)))
    .collect()
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    unsafe {
        command.as_std_mut().pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            #[cfg(target_os = "linux")]
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    Ok(())
}

#[cfg(windows)]
fn configure_process_group(command: &mut Command) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, CREATE_SUSPENDED};
    ensure_windows_console()?;
    command
        .as_std_mut()
        .creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_SUSPENDED);
    Ok(())
}

#[cfg(windows)]
static WINDOWS_CONSOLE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(windows)]
fn ensure_windows_console() -> std::io::Result<()> {
    use windows_sys::Win32::System::Console::{AllocConsole, GetConsoleProcessList};

    let _guard = WINDOWS_CONSOLE_LOCK
        .lock()
        .map_err(|_| std::io::Error::other("Windows console lock is poisoned"))?;
    let mut process_id = 0_u32;
    if unsafe { GetConsoleProcessList(&mut process_id, 1) } != 0 {
        return Ok(());
    }
    if unsafe { AllocConsole() } == 0 {
        if unsafe { GetConsoleProcessList(&mut process_id, 1) } != 0 {
            return Ok(());
        }
        return Err(std::io::Error::last_os_error());
    }
    if unsafe { GetConsoleProcessList(&mut process_id, 1) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn configure_process_group(_command: &mut Command) -> std::io::Result<()> {
    Err(std::io::Error::other("process groups are unsupported"))
}

struct ProcessGroupGuard {
    pid: u32,
    active: bool,
    #[cfg(windows)]
    job: WindowsJob,
}

impl ProcessGroupGuard {
    fn new(child: &mut tokio::process::Child) -> Result<Self, InstallerError> {
        let pid = child.id().ok_or_else(|| {
            InstallerError::new("installer_start_failed", "servers.installation_failed")
        })?;
        #[cfg(windows)]
        let job = match WindowsJob::assign_and_resume(child) {
            Ok(job) => job,
            Err(error) => {
                // The root was created suspended, so it cannot have launched an
                // uncontained Java/Git/Maven descendant. Kill it before returning.
                let _ = child.start_kill();
                return Err(InstallerError::internal(
                    "installer_containment_failed",
                    error,
                ));
            }
        };
        Ok(Self {
            pid,
            active: true,
            #[cfg(windows)]
            job,
        })
    }

    fn kill(&mut self) {
        if self.active {
            let _ = hard_kill_group(
                self.pid,
                #[cfg(windows)]
                Some(self.job.handle),
                #[cfg(not(windows))]
                None,
            );
            self.active = false;
        }
    }

    fn finish(&mut self) {
        // The official installer must not leave Git/Maven/Java descendants
        // behind after its root process exits.
        self.kill();
    }
}

impl Drop for ProcessGroupGuard {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(unix)]
fn hard_kill_group(pid: u32, _windows_job: Option<isize>) -> std::io::Result<()> {
    let result = unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
    if result == -1 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(windows)]
fn hard_kill_group(_pid: u32, windows_job: Option<isize>) -> std::io::Result<()> {
    use windows_sys::Win32::System::JobObjects::TerminateJobObject;
    let handle = windows_job.ok_or_else(|| std::io::Error::other("job object is unavailable"))?;
    if unsafe { TerminateJobObject(handle as _, 1) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn hard_kill_group(_pid: u32, _windows_job: Option<isize>) -> std::io::Result<()> {
    Err(std::io::Error::other("process groups are unsupported"))
}

#[cfg(windows)]
struct WindowsJob {
    handle: isize,
}

#[cfg(windows)]
impl WindowsJob {
    fn assign_and_resume(child: &tokio::process::Child) -> std::io::Result<Self> {
        use std::mem::{size_of, zeroed};
        use windows_sys::Win32::{
            Foundation::HANDLE,
            System::JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                SetInformationJobObject,
            },
        };
        let pid = child
            .id()
            .ok_or_else(|| std::io::Error::other("process has no identifier"))?;
        let process = child
            .raw_handle()
            .ok_or_else(|| std::io::Error::other("process has no Windows handle"))?
            as HANDLE;
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let job = Self {
            handle: job as isize,
        };
        let mut information: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
        information.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = unsafe {
            SetInformationJobObject(
                job.handle as _,
                JobObjectExtendedLimitInformation,
                &information as *const _ as _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if configured == 0 {
            return Err(std::io::Error::last_os_error());
        }
        if unsafe { AssignProcessToJobObject(job.handle as _, process) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        if let Err(error) = resume_suspended_primary_thread(pid) {
            let _ = hard_kill_group(pid, Some(job.handle));
            return Err(error);
        }
        Ok(job)
    }
}

#[cfg(windows)]
struct OwnedWindowsHandle(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for OwnedWindowsHandle {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        let _ = unsafe { CloseHandle(self.0) };
    }
}

#[cfg(windows)]
fn resume_suspended_primary_thread(pid: u32) -> std::io::Result<()> {
    use std::mem::size_of;
    use windows_sys::Win32::{
        Foundation::{ERROR_NO_MORE_FILES, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First,
                Thread32Next,
            },
            Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME},
        },
    };

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    let snapshot = OwnedWindowsHandle(snapshot);
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..THREADENTRY32::default()
    };
    if unsafe { Thread32First(snapshot.0, &mut entry) } == 0 {
        let error = std::io::Error::last_os_error();
        return if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "suspended process has no primary thread",
            ))
        } else {
            Err(error)
        };
    }

    let mut primary_thread_id = None;
    loop {
        if entry.th32OwnerProcessID == pid
            && primary_thread_id.replace(entry.th32ThreadID).is_some()
        {
            return Err(std::io::Error::other(
                "suspended process has multiple threads before containment",
            ));
        }
        if unsafe { Thread32Next(snapshot.0, &mut entry) } == 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
                return Err(error);
            }
            break;
        }
    }

    let thread_id = primary_thread_id.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "suspended process has no primary thread",
        )
    })?;
    let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, thread_id) };
    if thread.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    let thread = OwnedWindowsHandle(thread);
    let previous_suspend_count = unsafe { ResumeThread(thread.0) };
    if previous_suspend_count == u32::MAX {
        return Err(std::io::Error::last_os_error());
    }
    if previous_suspend_count != 1 {
        return Err(std::io::Error::other(format!(
            "unexpected primary thread suspend count {previous_suspend_count}"
        )));
    }
    Ok(())
}

#[cfg(windows)]
impl Drop for WindowsJob {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        unsafe { CloseHandle(self.handle as _) };
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, io::Write as _};

    use tempfile::tempdir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::*;
    use crate::services::installers::InstallerSources;

    fn fixture_context(responses: BTreeMap<String, Vec<u8>>) -> InstallContext {
        let base = Url::parse("https://fixtures.invalid/").unwrap();
        InstallContext::with_fixture_responses(InstallerSources::fixture(&base), responses).unwrap()
    }

    #[tokio::test]
    async fn fabric_requires_the_exact_game_and_loader_pair() {
        let url = "https://fixtures.invalid/fabric-meta/versions/loader/1.21.11/0.19.3";
        let context = fixture_context(BTreeMap::from([(
            url.to_string(),
            serde_json::to_vec(&serde_json::json!({
                "loader": {"version": "0.19.3"},
                "intermediary": {"version": "1.21.11"}
            }))
            .unwrap(),
        )]));
        validate_fabric_pair(&context, "1.21.11", "0.19.3")
            .await
            .unwrap();
        assert!(
            validate_fabric_pair(&context, "1.21.11", "0.19.2")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn quilt_requires_an_explicit_loader_from_official_metadata() {
        let url = "https://fixtures.invalid/quilt-meta/versions/loader/1.21.1";
        let context = fixture_context(BTreeMap::from([(
            url.to_string(),
            serde_json::to_vec(&serde_json::json!([{
                "loader": {"version": "0.29.0"},
                "hashed": {"version": "1.21.1"}
            }]))
            .unwrap(),
        )]));
        validate_quilt_pair(&context, "1.21.1", "0.29.0")
            .await
            .unwrap();
        assert!(
            validate_quilt_pair(&context, "1.21.1", "latest")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn purpur_pins_a_successful_numeric_build_and_its_digest() {
        let base = "https://fixtures.invalid/purpur/purpur/1.21.11";
        let context = fixture_context(BTreeMap::from([
            (
                base.to_string(),
                serde_json::to_vec(&serde_json::json!({
                    "project": "purpur",
                    "version": "1.21.11",
                    "builds": {"latest": "2568", "all": ["2567", "2568"]}
                }))
                .unwrap(),
            ),
            (
                format!("{base}/2568"),
                serde_json::to_vec(&serde_json::json!({
                    "project": "purpur",
                    "version": "1.21.11",
                    "build": "2568",
                    "result": "SUCCESS",
                    "md5": "b8d5402ef8e38bf60cabc6eeddb3fa18"
                }))
                .unwrap(),
            ),
        ]));
        let build = resolve_purpur_build(&context, "1.21.11", "2568")
            .await
            .unwrap();
        assert_eq!(build.md5, "b8d5402ef8e38bf60cabc6eeddb3fa18");
        assert!(
            resolve_purpur_build(&context, "1.21.11", "2569")
                .await
                .is_err()
        );
        assert!(required_numeric_build(&serde_json::json!({"build": "latest"}), "build").is_err());
    }

    #[tokio::test]
    async fn forge_and_neoforge_resolve_only_exact_checksummed_maven_artifacts() {
        let forge_checksum = "a".repeat(64);
        let neo_checksum = "b".repeat(64);
        let context = fixture_context(BTreeMap::from([
            (
                "https://fixtures.invalid/forge-maven/net/minecraftforge/forge/1.21.1-52.1.0/forge-1.21.1-52.1.0-installer.jar.sha256".to_string(),
                forge_checksum.clone().into_bytes(),
            ),
            (
                "https://fixtures.invalid/neoforge-maven/net/neoforged/neoforge/21.1.230/neoforge-21.1.230-installer.jar.sha256".to_string(),
                neo_checksum.clone().into_bytes(),
            ),
        ]));
        let forge = mod_loader_installer_source(&context, "forge", "1.21.1-52.1.0")
            .await
            .unwrap();
        assert_eq!(forge.sha256, forge_checksum);
        assert!(
            forge
                .url
                .path()
                .ends_with("forge-1.21.1-52.1.0-installer.jar")
        );
        let neo = mod_loader_installer_source(&context, "neoforge", "21.1.230")
            .await
            .unwrap();
        assert_eq!(neo.sha256, neo_checksum);
        validate_neoforge_version("1.21.1", "21.1.230").unwrap();
        assert!(validate_neoforge_version("1.21.4", "21.1.230").is_err());
        validate_neoforge_version("1.21", "21.0.167").unwrap();
        assert!(validate_neoforge_version("1.21", "21.1.230").is_err());
        validate_neoforge_version("26.1.2", "26.1.2.7").unwrap();
    }

    #[test]
    fn fabric_quilt_and_buildtools_are_immutable_maintainer_pins() {
        let sources = InstallerSources::official();
        let fabric =
            fabric_installer_source(&InstallContext::with_sources(sources.clone()).unwrap())
                .unwrap();
        assert_eq!(fabric.version, "1.1.1");
        assert_eq!(fabric.size, Some(209_151));
        assert_eq!(fabric.sha256, FABRIC_INSTALLER_SHA256);
        let quilt = quilt_installer_source(&InstallContext::with_sources(sources.clone()).unwrap())
            .unwrap();
        assert_eq!(quilt.version, "0.15.0");
        assert_eq!(quilt.sha256, QUILT_INSTALLER_SHA256);
        assert_eq!(sources.buildtools.version, "jenkins-200");
        assert_eq!(
            sources.buildtools.sha256,
            "b61fa90158f594ee95bea1a27399eb64d439b4c8ae9345bd4476a02ce49b06ff"
        );
        assert!(
            !sources
                .buildtools
                .url
                .as_str()
                .contains("lastSuccessfulBuild")
        );
    }

    #[tokio::test]
    #[ignore = "live pre-release smoke: downloads about 11 MiB from official provider repositories"]
    async fn live_official_maintainer_pins_match_remote_artifacts() {
        let directory = tempdir().unwrap();
        let context = InstallContext::official().unwrap();
        let sources = [
            (
                "fabric-installer.jar",
                fabric_installer_source(&context).unwrap(),
            ),
            (
                "quilt-installer.jar",
                quilt_installer_source(&context).unwrap(),
            ),
            ("BuildTools.jar", context.sources.buildtools.clone()),
        ];

        for (name, source) in sources {
            let downloaded = download_source(&context, &source, &directory.path().join(name))
                .await
                .unwrap();
            assert_eq!(downloaded.sha256, source.sha256);
            if let Some(expected_size) = source.size {
                assert_eq!(downloaded.size, expected_size);
            }
        }
    }

    #[tokio::test]
    #[ignore = "live pre-release smoke: queries official Minecraft loader metadata"]
    async fn live_official_loader_metadata_contracts_are_compatible() {
        let context = InstallContext::official().unwrap();

        validate_fabric_pair(&context, "1.21.8", "0.19.3")
            .await
            .unwrap();
        validate_quilt_pair(&context, "1.21.8", "0.20.0-beta.9")
            .await
            .unwrap();
        let purpur = resolve_purpur_build(&context, "1.21.8", "2497")
            .await
            .unwrap();
        validate_hex(&purpur.md5, 32).unwrap();
        for (loader, version) in [("forge", "1.21.1-52.1.0"), ("neoforge", "21.1.230")] {
            let source = mod_loader_installer_source(&context, loader, version)
                .await
                .unwrap();
            validate_source(&source).unwrap();
        }
    }

    #[tokio::test]
    async fn forge_installer_profile_must_match_the_requested_game() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("installer.jar");
        let file = File::create(&path).unwrap();
        let mut writer = ZipWriter::new(file);
        writer
            .start_file("install_profile.json", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(br#"{"minecraft":"1.21.1"}"#).unwrap();
        writer.finish().unwrap();
        validate_install_profile(&path, "1.21.1").await.unwrap();
        assert!(validate_install_profile(&path, "1.21.4").await.is_err());
    }

    #[test]
    fn launch_plans_never_execute_generated_shell_scripts() {
        let settings = serde_json::json!({
            "loader_version": "1.21.1-52.1.0",
            "max_memory_mb": 4096
        });
        let forge =
            launch_plan("minecraft-java-forge", &settings, 21, Some("1.21.1-52.1.0")).unwrap();
        assert!(forge.args[1].starts_with("@libraries/net/minecraftforge/forge/"));
        assert!(
            forge
                .args
                .iter()
                .all(|value| !value.ends_with(".sh") && !value.ends_with(".bat"))
        );
        let fabric_settings = serde_json::json!({
            "loader_version": "0.19.3",
            "max_memory_mb": 4096
        });
        let fabric = launch_plan(
            "minecraft-java-fabric",
            &fabric_settings,
            21,
            Some("0.19.3"),
        )
        .unwrap();
        assert_eq!(
            fabric.args,
            ["-Xmx4096M", "-jar", "fabric-server-launch.jar", "nogui"]
        );
    }

    #[test]
    fn argfiles_reject_nested_files_host_paths_and_java_agents() {
        assert!(validate_argfile_contents("-p libraries --add-modules ALL-MODULE-PATH").is_ok());
        for value in [
            "@user_jvm_args.txt",
            "-javaagent:evil.jar",
            "-XX:OnError=/bin/sh",
            "-cp ../../host.jar",
            "-cp /etc/passwd",
            "-DlibraryDirectory=/srv/other-instance",
            "-DlegacyClassPath=libraries/server.jar:/etc/passwd",
            "-DlibraryDirectory=C:\\other-instance",
        ] {
            assert!(
                validate_argfile_contents(value).is_err(),
                "accepted {value}"
            );
        }
    }

    #[test]
    fn versions_and_builds_refuse_floating_or_path_like_values() {
        for value in ["latest", "recommended", "../1.21", "1.21/evil", "x y", ""] {
            assert!(validate_version_value(value).is_err(), "accepted {value:?}");
        }
        for value in ["1.21.11", "26.1.2", "21.1.230", "0.20.0-beta.9"] {
            validate_version_value(value).unwrap();
        }
    }
}
