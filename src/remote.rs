use crate::config::project_dirs;
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
#[cfg_attr(feature = "smb", path = "remote_smb.rs")]
#[cfg_attr(not(feature = "smb"), path = "remote_smb_stub.rs")]
mod smb_impl;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteSource {
    SshConfig,
    UserToml,
    PluginAuto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteProtocol {
    Sftp,
    Smb,
    RemotePlugin,
}

#[derive(Debug, Clone)]
pub struct RemoteProfile {
    pub name: String,
    pub source: RemoteSource,
    pub kind: RemoteKind,
}

#[derive(Debug, Clone)]
pub enum RemoteKind {
    Sftp(SftpProfile),
    Smb(SmbProfile),
    RemotePlugin(RemotePluginProfile),
}

#[derive(Debug, Clone)]
pub struct SftpProfile {
    pub host: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub path: Option<String>,
    pub identity_file: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SmbProfile {
    pub host: String,
    pub user: Option<String>,
    pub password: Option<String>,
    pub workgroup: Option<String>,
    pub share: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RemotePluginProfile {
    pub plugin_id: String,
    pub scheme: String,
    pub config_json: String,
    pub path: Option<String>,
}

impl RemoteProtocol {
    /// Lowercase short name ("sftp", "smb").
    pub fn name(self) -> &'static str {
        match self {
            RemoteProtocol::Sftp => "sftp",
            RemoteProtocol::Smb => "smb",
            RemoteProtocol::RemotePlugin => "plugin",
        }
    }

    /// Display label ("SFTP", "SMB").
    pub fn label(self) -> &'static str {
        match self {
            RemoteProtocol::Sftp => "SFTP",
            RemoteProtocol::Smb => "SMB",
            RemoteProtocol::RemotePlugin => "PLUGIN",
        }
    }
}

impl RemoteProfile {
    pub fn protocol(&self) -> RemoteProtocol {
        match self.kind {
            RemoteKind::Sftp(_) => RemoteProtocol::Sftp,
            RemoteKind::Smb(_) => RemoteProtocol::Smb,
            RemoteKind::RemotePlugin(_) => RemoteProtocol::RemotePlugin,
        }
    }

    pub fn host_label(&self) -> String {
        match &self.kind {
            RemoteKind::Sftp(sftp) => sftp.host.clone().unwrap_or_else(|| self.name.clone()),
            RemoteKind::Smb(smb) => smb.host.clone(),
            RemoteKind::RemotePlugin(plugin) => {
                format!("{} ({})", plugin.plugin_id, plugin.scheme)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct RemoteEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub modified: Option<DateTime<Local>>,
    pub mode: u32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RemoteStats {
    pub files: usize,
    pub bytes: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ConnectionStore {
    #[serde(default)]
    sftp: Vec<SftpProfileToml>,
    #[serde(default)]
    smb: Vec<smb_impl::SmbProfileToml>,
    #[serde(default)]
    remote_plugin: Vec<RemotePluginProfileToml>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SftpProfileToml {
    name: String,
    #[serde(default)]
    host: Option<String>,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    identity_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RemotePluginProfileToml {
    name: String,
    plugin_id: String,
    #[serde(default)]
    scheme: Option<String>,
    #[serde(default)]
    config_json: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

pub fn connections_path() -> Result<PathBuf> {
    let dirs = project_dirs()?;
    let dir = dirs.preference_dir();
    fs::create_dir_all(dir)?;
    Ok(dir.join("connections.toml"))
}

pub fn load_profiles() -> Result<Vec<RemoteProfile>> {
    let start = std::time::Instant::now();
    let ssh_start = std::time::Instant::now();
    let mut out = load_ssh_profiles().unwrap_or_default();
    let ssh_count = out.len();
    crate::viewer::debug_log(&format!(
        "remote: loaded {} ssh config profile(s) in {:.3} ms",
        ssh_count,
        ssh_start.elapsed().as_secs_f64() * 1000.0
    ));
    let saved_start = std::time::Instant::now();
    let saved = load_saved_profiles()?;
    crate::viewer::debug_log(&format!(
        "remote: loaded {} saved profile(s) in {:.3} ms",
        saved.len(),
        saved_start.elapsed().as_secs_f64() * 1000.0
    ));
    out.extend(saved);
    let plugin_start = std::time::Instant::now();
    let plugin_profiles = load_auto_remote_plugin_profiles(&out)?;
    crate::viewer::debug_log(&format!(
        "remote: loaded {} auto plugin profile(s) in {:.3} ms",
        plugin_profiles.len(),
        plugin_start.elapsed().as_secs_f64() * 1000.0
    ));
    out.extend(plugin_profiles);
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    crate::viewer::debug_log(&format!(
        "remote: load_profiles completed with {} profile(s) in {:.3} ms",
        out.len(),
        start.elapsed().as_secs_f64() * 1000.0
    ));
    Ok(out)
}

fn load_auto_remote_plugin_profiles(existing: &[RemoteProfile]) -> Result<Vec<RemoteProfile>> {
    let plugins_dir = match crate::plugins::plugins_dir() {
        Ok(path) => path,
        Err(_) => return Ok(Vec::new()),
    };
    let discovered =
        match crate::remote_plugins::discover_remote_rust_plugin_manifests(&plugins_dir) {
            Ok(items) => items,
            Err(err) => {
                crate::viewer::debug_log(&format!(
                    "remote: native plugin discovery skipped: {err}"
                ));
                return Ok(Vec::new());
            }
        };

    let loaded_plugins = crate::remote_plugins::discover_remote_rust_plugins(&plugins_dir)
        .unwrap_or_default()
        .into_iter()
        .map(|plugin| (plugin.id, plugin.scheme))
        .collect::<std::collections::HashMap<_, _>>();

    let mut out = Vec::new();
    for plugin in discovered {
        let already_configured = existing.iter().any(|profile| {
            matches!(
                &profile.kind,
                RemoteKind::RemotePlugin(cfg) if cfg.plugin_id == plugin.id
            )
        });
        if already_configured {
            continue;
        }
        let scheme = loaded_plugins
            .get(&plugin.id)
            .cloned()
            .unwrap_or_else(|| plugin.id.clone());
        out.push(RemoteProfile {
            name: plugin.name,
            source: RemoteSource::PluginAuto,
            kind: RemoteKind::RemotePlugin(RemotePluginProfile {
                plugin_id: plugin.id,
                scheme,
                config_json: "{}".to_string(),
                path: Some("/".to_string()),
            }),
        });
    }
    Ok(out)
}

pub fn save_profile(profile: &RemoteProfile, old_name: Option<&str>) -> Result<()> {
    let path = connections_path()?;
    let mut store = if path.exists() {
        let text =
            fs::read_to_string(&path).with_context(|| format!("Reading {}", path.display()))?;
        toml::from_str::<ConnectionStore>(&text)
            .with_context(|| format!("Parsing {}", path.display()))?
    } else {
        ConnectionStore::default()
    };

    match &profile.kind {
        RemoteKind::Sftp(sftp) => {
            // Remove old entry by original name (rename case) and by new name (duplicate guard).
            if let Some(old) = old_name {
                store.sftp.retain(|p| !p.name.eq_ignore_ascii_case(old));
            }
            store
                .sftp
                .retain(|p| !p.name.eq_ignore_ascii_case(&profile.name));
            store.sftp.push(SftpProfileToml {
                name: profile.name.clone(),
                host: sftp.host.clone(),
                user: sftp.user.clone(),
                port: sftp.port,
                path: sftp.path.clone(),
                identity_file: sftp.identity_file.clone(),
            });
        }
        RemoteKind::Smb(smb) => {
            if let Some(old) = old_name {
                store.smb.retain(|p| !p.name.eq_ignore_ascii_case(old));
            }
            store
                .smb
                .retain(|p| !p.name.eq_ignore_ascii_case(&profile.name));
            store.smb.push(smb_impl::SmbProfileToml {
                name: profile.name.clone(),
                host: smb.host.clone(),
                user: smb.user.clone(),
                password: smb.password.clone(),
                workgroup: smb.workgroup.clone(),
                share: smb.share.clone(),
                path: smb.path.clone(),
            });
        }
        RemoteKind::RemotePlugin(plugin) => {
            if let Some(old) = old_name {
                store
                    .remote_plugin
                    .retain(|p| !p.name.eq_ignore_ascii_case(old));
            }
            store
                .remote_plugin
                .retain(|p| !p.name.eq_ignore_ascii_case(&profile.name));
            store.remote_plugin.push(RemotePluginProfileToml {
                name: profile.name.clone(),
                plugin_id: plugin.plugin_id.clone(),
                scheme: Some(plugin.scheme.clone()),
                config_json: Some(plugin.config_json.clone()),
                path: plugin.path.clone(),
            });
        }
    }

    let text = toml::to_string_pretty(&store).context("Serialising connections")?;
    fs::write(&path, text).with_context(|| format!("Writing {}", path.display()))?;
    Ok(())
}

pub fn resolve_initial_dir(profile: &RemoteProfile) -> Result<String> {
    let cwd = match &profile.kind {
        RemoteKind::Sftp(sftp) => resolve_sftp_initial_dir(profile, sftp),
        RemoteKind::Smb(smb) => Ok(smb.path.clone().unwrap_or_else(|| "/".into())),
        RemoteKind::RemotePlugin(plugin) => Ok(plugin.path.clone().unwrap_or_else(|| "/".into())),
    }?;
    Ok(normalize_remote_cwd(profile, &cwd))
}

pub fn normalize_remote_cwd(profile: &RemoteProfile, cwd: &str) -> String {
    match profile.protocol() {
        RemoteProtocol::Sftp => {
            if cwd.trim().is_empty() {
                "/".into()
            } else if cwd == "/" {
                "/".into()
            } else {
                cwd.trim_end_matches('/').to_string()
            }
        }
        RemoteProtocol::Smb => {
            if cwd.trim().is_empty() {
                "/".into()
            } else if cwd == "/" {
                "/".into()
            } else {
                cwd.trim_end_matches('/').to_string()
            }
        }
        RemoteProtocol::RemotePlugin => {
            if cwd.trim().is_empty() {
                "/".into()
            } else if cwd == "/" {
                "/".into()
            } else {
                cwd.trim_end_matches('/').to_string()
            }
        }
    }
}

pub fn prepare_connection<F>(
    profile: &RemoteProfile,
    show_hidden: bool,
    progress: &mut F,
    cancel: &Arc<AtomicBool>,
) -> Result<(String, Vec<RemoteEntry>)>
where
    F: FnMut(String),
{
    if cancel.load(Ordering::Relaxed) {
        bail!("Aborted");
    }
    progress("Resolving initial directory...".into());
    let cwd = resolve_initial_dir(profile)?;
    if cancel.load(Ordering::Relaxed) {
        bail!("Aborted");
    }
    progress(format!(
        "Listing {}...",
        if cwd == "/" { "root" } else { &cwd }
    ));
    let entries = match &profile.kind {
        RemoteKind::Sftp(_) => list_sftp_dir(profile, &cwd, show_hidden)?,
        RemoteKind::Smb(_) => smb_impl::list_smb_dir(profile, &cwd, show_hidden)?,
        RemoteKind::RemotePlugin(_) => remote_plugin_list_dir(profile, &cwd, show_hidden)?,
    };
    if cancel.load(Ordering::Relaxed) {
        bail!("Aborted");
    }
    Ok((cwd, entries))
}

pub fn display_path(profile: &RemoteProfile, cwd: &str) -> String {
    match profile.protocol() {
        RemoteProtocol::Sftp => format!("sftp://{}{}", profile.name, cwd),
        RemoteProtocol::Smb => {
            if let RemoteKind::Smb(smb) = &profile.kind {
                let share = smb.share.as_deref().unwrap_or("").trim_matches('/');
                if share.is_empty() {
                    format!("smb://{}{}", profile.name, cwd)
                } else if cwd == "/" {
                    format!("smb://{}/{}", profile.name, share)
                } else {
                    format!("smb://{}/{}{}", profile.name, share, cwd)
                }
            } else {
                format!("smb://{}{}", profile.name, cwd)
            }
        }
        RemoteProtocol::RemotePlugin => {
            if let RemoteKind::RemotePlugin(plugin) = &profile.kind {
                format!("{}://{}{}", plugin.scheme, profile.name, cwd)
            } else {
                format!("plugin://{}{}", profile.name, cwd)
            }
        }
    }
}

pub fn list_dir(profile: &RemoteProfile, cwd: &str, show_hidden: bool) -> Result<Vec<RemoteEntry>> {
    match &profile.kind {
        RemoteKind::Sftp(_) => list_sftp_dir(profile, cwd, show_hidden),
        RemoteKind::Smb(_) => smb_impl::list_smb_dir(profile, cwd, show_hidden),
        RemoteKind::RemotePlugin(_) => remote_plugin_list_dir(profile, cwd, show_hidden),
    }
}

/// Enumerate SMB shares available on the server (SMB only; fails for other kinds).
pub fn list_smb_shares(profile: &RemoteProfile) -> Result<Vec<String>> {
    smb_impl::list_smb_shares(profile)
}

pub fn join_remote(parent: &str, name: &str) -> String {
    if parent == "/" {
        format!("/{}", name)
    } else {
        format!("{}/{}", parent.trim_end_matches('/'), name)
    }
}

pub fn download_into_dir(
    profile: &RemoteProfile,
    remote_path: &str,
    local_dir: &Path,
    recursive: bool,
) -> Result<PathBuf> {
    match &profile.kind {
        RemoteKind::Sftp(_) => download_sftp_into_dir(profile, remote_path, local_dir, recursive),
        RemoteKind::Smb(_) => {
            smb_impl::download_smb_into_dir(profile, remote_path, local_dir, recursive)
        }
        RemoteKind::RemotePlugin(_) => {
            remote_plugin_download_into_dir(profile, remote_path, local_dir, recursive)
        }
    }
}

pub fn download_bulk_into_dir(
    profile: &RemoteProfile,
    remote_path: &str,
    local_dir: &Path,
) -> Result<PathBuf> {
    match &profile.kind {
        RemoteKind::Sftp(_) => download_sftp_bulk_into_dir(profile, remote_path, local_dir),
        RemoteKind::Smb(_) => {
            smb_impl::download_smb_into_dir(profile, remote_path, local_dir, true)
        }
        RemoteKind::RemotePlugin(_) => {
            remote_plugin_download_into_dir(profile, remote_path, local_dir, true)
        }
    }
}

pub fn upload_into_dir(
    profile: &RemoteProfile,
    local_path: &Path,
    remote_dir: &str,
    recursive: bool,
) -> Result<String> {
    match &profile.kind {
        RemoteKind::Sftp(_) => upload_sftp_into_dir(profile, local_path, remote_dir, recursive),
        RemoteKind::Smb(_) => {
            smb_impl::upload_smb_into_dir(profile, local_path, remote_dir, recursive)
        }
        RemoteKind::RemotePlugin(_) => {
            remote_plugin_upload_into_dir(profile, local_path, remote_dir, recursive)
        }
    }
}

pub fn upload_bulk_into_dir(
    profile: &RemoteProfile,
    local_path: &Path,
    remote_dir: &str,
) -> Result<String> {
    match &profile.kind {
        RemoteKind::Sftp(_) => upload_sftp_bulk_into_dir(profile, local_path, remote_dir),
        RemoteKind::Smb(_) => smb_impl::upload_smb_into_dir(profile, local_path, remote_dir, true),
        RemoteKind::RemotePlugin(_) => {
            remote_plugin_upload_into_dir(profile, local_path, remote_dir, true)
        }
    }
}

pub fn rename_path(profile: &RemoteProfile, old_path: &str, new_path: &str) -> Result<()> {
    match &profile.kind {
        RemoteKind::Sftp(_) => {
            run_sftp_batch(
                profile,
                &[format!(
                    "rename {} {}",
                    batch_quote(old_path),
                    batch_quote(new_path)
                )],
            )?;
            Ok(())
        }
        RemoteKind::Smb(smb) => smb_impl::smb_rename(smb, old_path, new_path),
        RemoteKind::RemotePlugin(_) => {
            let local_temp = std::env::temp_dir();
            let downloaded =
                remote_plugin_download_into_dir(profile, old_path, &local_temp, false)?;
            let target_parent = Path::new(new_path)
                .parent()
                .unwrap_or_else(|| Path::new("/"))
                .to_string_lossy()
                .into_owned();
            let uploaded =
                remote_plugin_upload_into_dir(profile, &downloaded, &target_parent, false)?;
            if normalize_remote_cwd(profile, &uploaded) != normalize_remote_cwd(profile, new_path) {
                return Err(anyhow::anyhow!(
                    "remote plugin rename fallback uploaded '{}' instead of '{}'",
                    uploaded,
                    new_path
                ));
            }
            remote_plugin_delete_path(profile, old_path, false)
        }
    }
}

pub fn make_dir(profile: &RemoteProfile, remote_path: &str) -> Result<()> {
    match &profile.kind {
        RemoteKind::Sftp(_) => {
            run_sftp_batch(profile, &[format!("mkdir {}", batch_quote(remote_path))])?;
            Ok(())
        }
        RemoteKind::Smb(smb) => smb_impl::smb_mkdir(smb, remote_path),
        RemoteKind::RemotePlugin(_) => remote_plugin_make_dir(profile, remote_path),
    }
}

pub fn delete_path(profile: &RemoteProfile, remote_path: &str, is_dir: bool) -> Result<()> {
    match &profile.kind {
        RemoteKind::Sftp(_) => {
            if is_dir {
                delete_sftp_dir_recursive(profile, remote_path)?;
            } else {
                run_sftp_batch(profile, &[format!("rm {}", batch_quote(remote_path))])?;
            }
            Ok(())
        }
        RemoteKind::Smb(_) => {
            if is_dir {
                smb_impl::delete_smb_dir_recursive(profile, remote_path)
            } else {
                let RemoteKind::Smb(smb) = &profile.kind else {
                    unreachable!()
                };
                smb_impl::smb_delete_file(smb, remote_path)
            }
        }
        RemoteKind::RemotePlugin(_) => remote_plugin_delete_path(profile, remote_path, is_dir),
    }
}

pub fn download_to_temp(
    profile: &RemoteProfile,
    remote_path: &str,
    recursive: bool,
) -> Result<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let base =
        std::env::temp_dir()
            .join("kkc-remote")
            .join(format!("{}-{}", std::process::id(), stamp));
    download_into_dir(profile, remote_path, &base, recursive)
}

#[allow(dead_code)]
pub fn remote_stats(
    profile: &RemoteProfile,
    remote_path: &str,
    is_dir: bool,
) -> Result<RemoteStats> {
    match &profile.kind {
        RemoteKind::Sftp(_) => remote_sftp_stats(profile, remote_path, is_dir),
        RemoteKind::Smb(_) => smb_impl::remote_smb_stats(profile, remote_path, is_dir),
        RemoteKind::RemotePlugin(_) => remote_plugin_stats(profile, remote_path, is_dir),
    }
}

pub fn scan_remote_stats<F>(
    profile: &RemoteProfile,
    remote_path: &str,
    is_dir: bool,
    progress: &mut F,
    cancel: &Arc<AtomicBool>,
) -> Result<RemoteStats>
where
    F: FnMut(RemoteStats),
{
    if cancel.load(Ordering::Relaxed) {
        bail!("Aborted");
    }
    match &profile.kind {
        RemoteKind::Sftp(_) => scan_sftp_stats(profile, remote_path, is_dir, progress, cancel),
        RemoteKind::Smb(_) => {
            smb_impl::scan_smb_stats(profile, remote_path, is_dir, progress, cancel)
        }
        RemoteKind::RemotePlugin(_) => {
            remote_plugin_scan_stats(profile, remote_path, is_dir, progress, cancel)
        }
    }
}

pub fn download_with_progress<F>(
    profile: &RemoteProfile,
    remote_path: &str,
    local_dir: &Path,
    recursive: bool,
    progress: &mut F,
) -> Result<PathBuf>
where
    F: FnMut(&str, u64) -> bool,
{
    match &profile.kind {
        RemoteKind::Sftp(_) => {
            download_sftp_with_progress(profile, remote_path, local_dir, recursive, progress)
        }
        RemoteKind::Smb(_) => smb_impl::download_smb_with_progress(
            profile,
            remote_path,
            local_dir,
            recursive,
            progress,
        ),
        RemoteKind::RemotePlugin(_) => {
            let path = remote_plugin_download_into_dir(profile, remote_path, local_dir, recursive)?;
            let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if !progress(remote_path, size) {
                bail!("Aborted");
            }
            Ok(path)
        }
    }
}

pub fn upload_with_progress<F>(
    profile: &RemoteProfile,
    local_path: &Path,
    remote_dir: &str,
    recursive: bool,
    progress: &mut F,
) -> Result<String>
where
    F: FnMut(&str, u64) -> bool,
{
    match &profile.kind {
        RemoteKind::Sftp(_) => {
            upload_sftp_with_progress(profile, local_path, remote_dir, recursive, progress)
        }
        RemoteKind::Smb(_) => {
            smb_impl::upload_smb_with_progress(profile, local_path, remote_dir, recursive, progress)
        }
        RemoteKind::RemotePlugin(_) => {
            let uploaded =
                remote_plugin_upload_into_dir(profile, local_path, remote_dir, recursive)?;
            let size = fs::metadata(local_path).map(|m| m.len()).unwrap_or(0);
            if !progress(&uploaded, size) {
                bail!("Aborted");
            }
            Ok(uploaded)
        }
    }
}

fn remote_plugin_profile(profile: &RemoteProfile) -> Result<&RemotePluginProfile> {
    let RemoteKind::RemotePlugin(plugin) = &profile.kind else {
        bail!("Profile '{}' is not a remote plugin profile", profile.name);
    };
    Ok(plugin)
}

fn remote_plugin_module(profile: &RemoteProfile) -> Result<kkc_plugin_api::RemotePluginModRef> {
    let plugin = remote_plugin_profile(profile)?;
    let module = crate::remote_plugins::load_remote_plugin(&plugin.plugin_id)?;
    module.set_debug_log()(remote_plugin_debug_log as *const () as usize);
    Ok(module)
}

extern "C" fn remote_plugin_debug_log(message: abi_stable::std_types::RString) {
    crate::viewer::debug_log(&format!("remote-plugin: {}", message));
}

fn remote_plugin_error(err: abi_stable::std_types::RString) -> anyhow::Error {
    anyhow::anyhow!(err.to_string())
}

#[allow(dead_code)]
pub fn remote_plugin_auth_start(plugin_id: &str, config_json: &str) -> Result<String> {
    let module = crate::remote_plugins::load_remote_plugin(plugin_id)?;
    module.set_debug_log()(remote_plugin_debug_log as *const () as usize);
    let call = module.auth_start()(config_json.into());
    match call {
        abi_stable::std_types::RResult::ROk(session) => Ok(session.to_string()),
        abi_stable::std_types::RResult::RErr(err) => Err(remote_plugin_error(err)),
    }
}

#[allow(dead_code)]
pub fn remote_plugin_auth_complete(
    plugin_id: &str,
    config_json: &str,
    auth_session_json: &str,
    input: &str,
) -> Result<String> {
    let module = crate::remote_plugins::load_remote_plugin(plugin_id)?;
    module.set_debug_log()(remote_plugin_debug_log as *const () as usize);
    let call = module.auth_complete()(config_json.into(), auth_session_json.into(), input.into());
    match call {
        abi_stable::std_types::RResult::ROk(config) => Ok(config.to_string()),
        abi_stable::std_types::RResult::RErr(err) => Err(remote_plugin_error(err)),
    }
}

fn remote_plugin_list_dir(
    profile: &RemoteProfile,
    cwd: &str,
    show_hidden: bool,
) -> Result<Vec<RemoteEntry>> {
    let plugin = remote_plugin_profile(profile)?;
    let module = remote_plugin_module(profile)?;
    let call = module.list_dir()(plugin.config_json.as_str().into(), cwd.into(), show_hidden);
    let entries = match call {
        abi_stable::std_types::RResult::ROk(entries) => entries,
        abi_stable::std_types::RResult::RErr(err) => return Err(remote_plugin_error(err)),
    };

    let mut out = entries
        .into_iter()
        .map(|entry| {
            let modified = if entry.modified_unix > 0 {
                Local.timestamp_opt(entry.modified_unix, 0).single()
            } else {
                None
            };
            RemoteEntry {
                name: entry.name.to_string(),
                path: normalize_remote_cwd(profile, entry.path.as_str()),
                is_dir: entry.is_dir,
                is_symlink: entry.is_symlink,
                size: entry.size,
                modified,
                mode: entry.mode,
            }
        })
        .collect::<Vec<_>>();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

fn remote_plugin_download_into_dir(
    profile: &RemoteProfile,
    remote_path: &str,
    local_dir: &Path,
    recursive: bool,
) -> Result<PathBuf> {
    let plugin = remote_plugin_profile(profile)?;
    let module = remote_plugin_module(profile)?;
    let local_dir_string = local_dir.to_string_lossy();
    let call = module.download_into_dir()(
        plugin.config_json.as_str().into(),
        remote_path.into(),
        local_dir_string.as_ref().into(),
        recursive,
    );
    let local_path = match call {
        abi_stable::std_types::RResult::ROk(path) => path.to_string(),
        abi_stable::std_types::RResult::RErr(err) => return Err(remote_plugin_error(err)),
    };
    Ok(PathBuf::from(local_path))
}

fn remote_plugin_upload_into_dir(
    profile: &RemoteProfile,
    local_path: &Path,
    remote_dir: &str,
    recursive: bool,
) -> Result<String> {
    let plugin = remote_plugin_profile(profile)?;
    let module = remote_plugin_module(profile)?;
    let local_string = local_path.to_string_lossy();
    let call = module.upload_into_dir()(
        plugin.config_json.as_str().into(),
        local_string.as_ref().into(),
        remote_dir.into(),
        recursive,
    );
    match call {
        abi_stable::std_types::RResult::ROk(path) => {
            Ok(normalize_remote_cwd(profile, path.as_str()))
        }
        abi_stable::std_types::RResult::RErr(err) => Err(remote_plugin_error(err)),
    }
}

fn remote_plugin_delete_path(
    profile: &RemoteProfile,
    remote_path: &str,
    is_dir: bool,
) -> Result<()> {
    let plugin = remote_plugin_profile(profile)?;
    let module = remote_plugin_module(profile)?;
    let call = module.delete_path()(
        plugin.config_json.as_str().into(),
        remote_path.into(),
        is_dir,
    );
    match call {
        abi_stable::std_types::RResult::ROk(()) => Ok(()),
        abi_stable::std_types::RResult::RErr(err) => Err(remote_plugin_error(err)),
    }
}

fn remote_plugin_make_dir(profile: &RemoteProfile, remote_path: &str) -> Result<()> {
    let plugin = remote_plugin_profile(profile)?;
    let module = remote_plugin_module(profile)?;
    let call = module.make_dir()(plugin.config_json.as_str().into(), remote_path.into());
    match call {
        abi_stable::std_types::RResult::ROk(()) => Ok(()),
        abi_stable::std_types::RResult::RErr(err) => Err(remote_plugin_error(err)),
    }
}

fn remote_plugin_stats(
    profile: &RemoteProfile,
    remote_path: &str,
    is_dir: bool,
) -> Result<RemoteStats> {
    if !is_dir {
        let parent = Path::new(remote_path).parent().unwrap_or(Path::new("/"));
        let file_name = Path::new(remote_path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let entries = remote_plugin_list_dir(profile, &parent.to_string_lossy(), true)?;
        let size = entries
            .into_iter()
            .find(|e| e.name == file_name)
            .map(|e| e.size)
            .unwrap_or(0);
        return Ok(RemoteStats {
            files: 1,
            bytes: size,
        });
    }
    remote_plugin_dir_stats_recursive(profile, remote_path)
}

fn remote_plugin_dir_stats_recursive(
    profile: &RemoteProfile,
    remote_path: &str,
) -> Result<RemoteStats> {
    let mut stats = RemoteStats::default();
    for child in remote_plugin_list_dir(profile, remote_path, true)? {
        if child.is_dir {
            let sub = remote_plugin_dir_stats_recursive(profile, &child.path)?;
            stats.files += sub.files;
            stats.bytes += sub.bytes;
        } else {
            stats.files += 1;
            stats.bytes += child.size;
        }
    }
    Ok(stats)
}

fn remote_plugin_scan_stats<F>(
    profile: &RemoteProfile,
    remote_path: &str,
    is_dir: bool,
    progress: &mut F,
    cancel: &Arc<AtomicBool>,
) -> Result<RemoteStats>
where
    F: FnMut(RemoteStats),
{
    if cancel.load(Ordering::Relaxed) {
        bail!("Aborted");
    }
    if !is_dir {
        let stats = remote_plugin_stats(profile, remote_path, false)?;
        progress(stats);
        return Ok(stats);
    }
    remote_plugin_scan_dir_recursive(profile, remote_path, progress, cancel)
}

fn remote_plugin_scan_dir_recursive<F>(
    profile: &RemoteProfile,
    remote_path: &str,
    progress: &mut F,
    cancel: &Arc<AtomicBool>,
) -> Result<RemoteStats>
where
    F: FnMut(RemoteStats),
{
    let mut stats = RemoteStats::default();
    for child in remote_plugin_list_dir(profile, remote_path, true)? {
        if cancel.load(Ordering::Relaxed) {
            bail!("Aborted");
        }
        if child.is_dir {
            let sub = remote_plugin_scan_dir_recursive(profile, &child.path, progress, cancel)?;
            stats.files += sub.files;
            stats.bytes += sub.bytes;
        } else {
            let delta = RemoteStats {
                files: 1,
                bytes: child.size,
            };
            stats.files += delta.files;
            stats.bytes += delta.bytes;
            progress(delta);
        }
    }
    Ok(stats)
}

fn load_saved_profiles() -> Result<Vec<RemoteProfile>> {
    let path = connections_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path).with_context(|| format!("Reading {}", path.display()))?;
    let store = toml::from_str::<ConnectionStore>(&text)
        .with_context(|| format!("Parsing {}", path.display()))?;
    let mut out = Vec::new();
    out.extend(store.sftp.into_iter().map(|p| RemoteProfile {
        name: p.name,
        source: RemoteSource::UserToml,
        kind: RemoteKind::Sftp(SftpProfile {
            host: p.host,
            user: p.user,
            port: p.port,
            path: p.path,
            identity_file: p.identity_file,
        }),
    }));
    out.extend(store.smb.into_iter().map(|p| RemoteProfile {
        name: p.name,
        source: RemoteSource::UserToml,
        kind: RemoteKind::Smb(SmbProfile {
            host: p.host,
            user: p.user,
            password: p.password,
            workgroup: p.workgroup,
            share: p.share,
            path: p.path,
        }),
    }));
    out.extend(store.remote_plugin.into_iter().map(|p| {
        let scheme = p.scheme.unwrap_or_else(|| p.plugin_id.clone());
        let config_json = p
            .config_json
            .filter(|cfg| !cfg.trim().is_empty())
            .unwrap_or_else(|| "{}".to_string());
        RemoteProfile {
            name: p.name,
            source: RemoteSource::UserToml,
            kind: RemoteKind::RemotePlugin(RemotePluginProfile {
                plugin_id: p.plugin_id,
                scheme,
                config_json,
                path: p.path,
            }),
        }
    }));
    Ok(out)
}

fn load_ssh_profiles() -> Result<Vec<RemoteProfile>> {
    let config_path = directories::UserDirs::new()
        .map(|u| u.home_dir().join(".ssh/config"))
        .unwrap_or_else(|| PathBuf::from(".ssh/config"));
    if !config_path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&config_path)
        .with_context(|| format!("Reading {}", config_path.display()))?;

    let mut out = Vec::new();
    let mut current_hosts: Vec<String> = Vec::new();
    let mut host_name: Option<String> = None;
    let mut user: Option<String> = None;
    let mut port: Option<u16> = None;
    let mut identity_file: Option<String> = None;

    let flush = |out: &mut Vec<RemoteProfile>,
                 current_hosts: &mut Vec<String>,
                 host_name: &Option<String>,
                 user: &Option<String>,
                 port: Option<u16>,
                 identity_file: &Option<String>| {
        for alias in current_hosts.drain(..) {
            out.push(RemoteProfile {
                name: alias,
                source: RemoteSource::SshConfig,
                kind: RemoteKind::Sftp(SftpProfile {
                    host: host_name.clone(),
                    user: user.clone(),
                    port,
                    path: None,
                    identity_file: identity_file.clone(),
                }),
            });
        }
    };

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else {
            continue;
        };
        let rest = parts.collect::<Vec<_>>().join(" ");
        match key.to_ascii_lowercase().as_str() {
            "host" => {
                flush(
                    &mut out,
                    &mut current_hosts,
                    &host_name,
                    &user,
                    port,
                    &identity_file,
                );
                host_name = None;
                user = None;
                port = None;
                identity_file = None;
                current_hosts = rest
                    .split_whitespace()
                    .filter(|alias| {
                        !alias.contains('*') && !alias.contains('?') && !alias.starts_with('!')
                    })
                    .map(|s| s.to_string())
                    .collect();
            }
            "hostname" => host_name = Some(rest),
            "user" => user = Some(rest),
            "port" => port = rest.parse::<u16>().ok(),
            "identityfile" => identity_file = Some(rest),
            _ => {}
        }
    }
    flush(
        &mut out,
        &mut current_hosts,
        &host_name,
        &user,
        port,
        &identity_file,
    );
    Ok(out)
}

fn resolve_sftp_initial_dir(profile: &RemoteProfile, sftp: &SftpProfile) -> Result<String> {
    if let Some(path) = sftp.path.as_ref().filter(|s| !s.trim().is_empty()) {
        return Ok(path.clone());
    }
    let out = run_sftp_batch(profile, &["pwd".into()])?;
    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("Remote working directory: ") {
            return Ok(rest.trim().to_string());
        }
    }
    Ok("/".into())
}

fn list_sftp_dir(
    profile: &RemoteProfile,
    cwd: &str,
    show_hidden: bool,
) -> Result<Vec<RemoteEntry>> {
    let out = run_sftp_batch(
        profile,
        &[format!("cd {}", batch_quote(cwd)), "ls -la".into()],
    )?;
    let mut entries = Vec::new();
    for line in out.lines() {
        if let Some(mut entry) = parse_ls_line(line) {
            if entry.name == "." || entry.name == ".." {
                continue;
            }
            if !show_hidden && entry.name.starts_with('.') {
                continue;
            }
            entry.path = join_remote(cwd, &entry.name);
            entries.push(entry);
        }
    }
    Ok(entries)
}

fn download_sftp_into_dir(
    profile: &RemoteProfile,
    remote_path: &str,
    local_dir: &Path,
    recursive: bool,
) -> Result<PathBuf> {
    let name = Path::new(remote_path)
        .file_name()
        .context("remote path has no file name")?;
    let local_target = local_dir.join(name);
    download_sftp_path::<fn(&str, u64) -> bool>(
        profile,
        remote_path,
        &local_target,
        recursive,
        None,
    )?;
    Ok(local_target)
}

fn download_sftp_bulk_into_dir(
    profile: &RemoteProfile,
    remote_path: &str,
    local_dir: &Path,
) -> Result<PathBuf> {
    let name = Path::new(remote_path)
        .file_name()
        .context("remote path has no file name")?;
    let local_target = local_dir.join(name);
    if let Some(parent) = local_target.parent() {
        fs::create_dir_all(parent)?;
    }
    run_sftp_batch(
        profile,
        &[format!(
            "get -r {} {}",
            batch_quote(remote_path),
            batch_quote(&local_target.to_string_lossy())
        )],
    )?;
    Ok(local_target)
}

fn upload_sftp_into_dir(
    profile: &RemoteProfile,
    local_path: &Path,
    remote_dir: &str,
    recursive: bool,
) -> Result<String> {
    let name = local_path
        .file_name()
        .context("local path has no file name")?;
    let remote_target = join_remote(remote_dir, &name.to_string_lossy());
    upload_sftp_path::<fn(&str, u64) -> bool>(
        profile,
        local_path,
        &remote_target,
        recursive,
        None,
    )?;
    Ok(remote_target)
}

fn upload_sftp_bulk_into_dir(
    profile: &RemoteProfile,
    local_path: &Path,
    remote_dir: &str,
) -> Result<String> {
    let name = local_path
        .file_name()
        .context("local path has no file name")?;
    let remote_target = join_remote(remote_dir, &name.to_string_lossy());
    run_sftp_batch(
        profile,
        &[format!(
            "put -r {} {}",
            batch_quote(&local_path.to_string_lossy()),
            batch_quote(&remote_target)
        )],
    )?;
    Ok(remote_target)
}

fn remote_sftp_stats(
    profile: &RemoteProfile,
    remote_path: &str,
    is_dir: bool,
) -> Result<RemoteStats> {
    if !is_dir {
        let parent = Path::new(remote_path).parent().unwrap_or(Path::new("/"));
        let file_name = Path::new(remote_path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let entries = list_sftp_dir(profile, &parent.to_string_lossy(), true)?;
        let size = entries
            .into_iter()
            .find(|e| e.name == file_name)
            .map(|e| e.size)
            .unwrap_or(0);
        return Ok(RemoteStats {
            files: 1,
            bytes: size,
        });
    }
    remote_sftp_dir_stats_recursive(profile, remote_path)
}

fn scan_sftp_stats<F>(
    profile: &RemoteProfile,
    remote_path: &str,
    is_dir: bool,
    progress: &mut F,
    cancel: &Arc<AtomicBool>,
) -> Result<RemoteStats>
where
    F: FnMut(RemoteStats),
{
    if cancel.load(Ordering::Relaxed) {
        bail!("Aborted");
    }
    if !is_dir {
        let stats = remote_sftp_stats(profile, remote_path, false)?;
        progress(stats);
        return Ok(stats);
    }
    scan_sftp_dir_recursive(profile, remote_path, progress, cancel)
}

fn download_sftp_with_progress<F>(
    profile: &RemoteProfile,
    remote_path: &str,
    local_dir: &Path,
    recursive: bool,
    progress: &mut F,
) -> Result<PathBuf>
where
    F: FnMut(&str, u64) -> bool,
{
    let name = Path::new(remote_path)
        .file_name()
        .context("remote path has no file name")?;
    let local_target = local_dir.join(name);
    download_sftp_path(
        profile,
        remote_path,
        &local_target,
        recursive,
        Some(progress),
    )?;
    Ok(local_target)
}

fn upload_sftp_with_progress<F>(
    profile: &RemoteProfile,
    local_path: &Path,
    remote_dir: &str,
    recursive: bool,
    progress: &mut F,
) -> Result<String>
where
    F: FnMut(&str, u64) -> bool,
{
    let name = local_path
        .file_name()
        .context("local path has no file name")?;
    let remote_target = join_remote(remote_dir, &name.to_string_lossy());
    upload_sftp_path(
        profile,
        local_path,
        &remote_target,
        recursive,
        Some(progress),
    )?;
    Ok(remote_target)
}

fn delete_sftp_dir_recursive(profile: &RemoteProfile, remote_path: &str) -> Result<()> {
    let children = list_sftp_dir(profile, remote_path, true)?;
    for child in children {
        if child.is_dir {
            delete_sftp_dir_recursive(profile, &child.path)?;
        } else {
            run_sftp_batch(profile, &[format!("rm {}", batch_quote(&child.path))])?;
        }
    }
    run_sftp_batch(profile, &[format!("rmdir {}", batch_quote(remote_path))])?;
    Ok(())
}

fn remote_sftp_dir_stats_recursive(
    profile: &RemoteProfile,
    remote_path: &str,
) -> Result<RemoteStats> {
    let mut stats = RemoteStats::default();
    for child in list_sftp_dir(profile, remote_path, true)? {
        if child.is_dir {
            let sub = remote_sftp_dir_stats_recursive(profile, &child.path)?;
            stats.files += sub.files;
            stats.bytes += sub.bytes;
        } else {
            stats.files += 1;
            stats.bytes += child.size;
        }
    }
    Ok(stats)
}

fn scan_sftp_dir_recursive<F>(
    profile: &RemoteProfile,
    remote_path: &str,
    progress: &mut F,
    cancel: &Arc<AtomicBool>,
) -> Result<RemoteStats>
where
    F: FnMut(RemoteStats),
{
    let mut stats = RemoteStats::default();
    for child in list_sftp_dir(profile, remote_path, true)? {
        if cancel.load(Ordering::Relaxed) {
            bail!("Aborted");
        }
        if child.is_dir {
            let sub = scan_sftp_dir_recursive(profile, &child.path, progress, cancel)?;
            stats.files += sub.files;
            stats.bytes += sub.bytes;
        } else {
            let delta = RemoteStats {
                files: 1,
                bytes: child.size,
            };
            stats.files += delta.files;
            stats.bytes += delta.bytes;
            progress(delta);
        }
    }
    Ok(stats)
}

fn download_sftp_path<F>(
    profile: &RemoteProfile,
    remote_path: &str,
    local_target: &Path,
    recursive: bool,
    mut progress: Option<&mut F>,
) -> Result<()>
where
    F: FnMut(&str, u64) -> bool,
{
    if !recursive {
        if let Some(parent) = local_target.parent() {
            fs::create_dir_all(parent)?;
        }
        run_sftp_batch(
            profile,
            &[format!(
                "get {} {}",
                batch_quote(remote_path),
                batch_quote(&local_target.to_string_lossy())
            )],
        )?;
        if let Some(cb) = progress.as_mut()
            && !cb(
                remote_path,
                fs::metadata(local_target).map(|m| m.len()).unwrap_or(0),
            )
        {
            bail!("Aborted");
        }
        return Ok(());
    }

    fs::create_dir_all(local_target)?;
    for child in list_sftp_dir(profile, remote_path, true)? {
        let child_local = local_target.join(&child.name);
        if child.is_dir {
            download_sftp_path(
                profile,
                &child.path,
                &child_local,
                true,
                progress.as_deref_mut(),
            )?;
        } else {
            download_sftp_path(
                profile,
                &child.path,
                &child_local,
                false,
                progress.as_deref_mut(),
            )?;
        }
    }
    Ok(())
}

fn upload_sftp_path<F>(
    profile: &RemoteProfile,
    local_path: &Path,
    remote_target: &str,
    recursive: bool,
    mut progress: Option<&mut F>,
) -> Result<()>
where
    F: FnMut(&str, u64) -> bool,
{
    if !recursive || !local_path.is_dir() {
        run_sftp_batch(
            profile,
            &[format!(
                "put {} {}",
                batch_quote(&local_path.to_string_lossy()),
                batch_quote(remote_target)
            )],
        )?;
        if let Some(cb) = progress.as_mut()
            && !cb(
                &local_path.to_string_lossy(),
                fs::metadata(local_path).map(|m| m.len()).unwrap_or(0),
            )
        {
            bail!("Aborted");
        }
        return Ok(());
    }

    let _ = run_sftp_batch(profile, &[format!("mkdir {}", batch_quote(remote_target))]);
    for entry in fs::read_dir(local_path)? {
        let entry = entry?;
        let child_local = entry.path();
        let child_remote = join_remote(remote_target, &entry.file_name().to_string_lossy());
        if child_local.is_dir() {
            upload_sftp_path(
                profile,
                &child_local,
                &child_remote,
                true,
                progress.as_deref_mut(),
            )?;
        } else {
            upload_sftp_path(
                profile,
                &child_local,
                &child_remote,
                false,
                progress.as_deref_mut(),
            )?;
        }
    }
    Ok(())
}

fn run_sftp_batch(profile: &RemoteProfile, commands: &[String]) -> Result<String> {
    let RemoteKind::Sftp(sftp) = &profile.kind else {
        bail!("Profile is not SFTP");
    };
    let mut cmd = Command::new("sftp");
    cmd.arg("-q").arg("-b").arg("-");
    // Prevent sftp from opening /dev/tty for password/passphrase prompts,
    // which would corrupt the TUI. Connections must use key-based auth
    // (agent or identity file). Accept new host keys automatically so that
    // first-time connections to a TOML-defined server don't hang.
    cmd.arg("-o").arg("BatchMode=yes");
    cmd.arg("-o").arg("StrictHostKeyChecking=accept-new");
    if let Some(port) = sftp.port {
        cmd.arg("-P").arg(port.to_string());
    }
    if let Some(identity) = sftp.identity_file.as_ref().filter(|s| !s.trim().is_empty()) {
        cmd.arg("-i").arg(expand_tilde(identity));
        cmd.arg("-o").arg("IdentitiesOnly=yes");
    }
    cmd.arg(remote_target(profile));
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().context("Launching sftp")?;
    {
        let stdin = child.stdin.as_mut().context("Opening sftp stdin")?;
        for command in commands {
            writeln!(stdin, "{command}")?;
        }
    }
    let output = child.wait_with_output().context("Waiting for sftp")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{}", stderr.trim().if_empty("sftp failed"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn remote_target(profile: &RemoteProfile) -> String {
    let RemoteKind::Sftp(sftp) = &profile.kind else {
        return profile.name.clone();
    };
    match (&sftp.user, &sftp.host) {
        (Some(user), Some(host)) => format!("{}@{}", user, host),
        (Some(user), None) => format!("{}@{}", user, profile.name),
        (None, Some(host)) => host.clone(),
        (None, None) => profile.name.clone(),
    }
}

fn batch_quote(path: &str) -> String {
    if path.contains(' ') || path.contains('"') {
        format!("\"{}\"", path.replace('"', "\\\""))
    } else {
        path.to_string()
    }
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = directories::UserDirs::new().map(|u| u.home_dir().to_path_buf())
    {
        return home.join(rest);
    }
    PathBuf::from(path)
}

fn parse_ls_line(line: &str) -> Option<RemoteEntry> {
    if line.is_empty()
        || line.starts_with("sftp>")
        || line.starts_with("Connected to")
        || line.starts_with("Fetching")
        || line.starts_with("Remote working directory:")
        || line.starts_with("total ")
    {
        return None;
    }
    let mode_txt = line.split_whitespace().next()?;
    if mode_txt.len() < 10 {
        return None;
    }
    let file_type = mode_txt.chars().next()?;
    if !matches!(file_type, '-' | 'd' | 'l') {
        return None;
    }
    let parts = line.split_whitespace().collect::<Vec<_>>();
    if parts.len() < 8 {
        return None;
    }
    let size_idx = if parts.len() >= 9 { 4 } else { return None };
    let month_idx = size_idx + 1;
    let day_idx = size_idx + 2;
    let time_or_year_idx = size_idx + 3;
    let name_idx = size_idx + 4;
    if parts.len() <= name_idx {
        return None;
    }
    let size = parts[size_idx].parse::<u64>().ok().unwrap_or(0);
    let raw_name = parts[name_idx..].join(" ");
    let name = raw_name.split(" -> ").next()?.to_string();
    let modified = parse_sftp_time(parts[month_idx], parts[day_idx], parts[time_or_year_idx]);
    Some(RemoteEntry {
        name,
        path: String::new(),
        is_dir: file_type == 'd',
        is_symlink: file_type == 'l',
        size,
        modified,
        mode: parse_mode_bits(mode_txt),
    })
}

fn parse_sftp_time(month: &str, day: &str, time_or_year: &str) -> Option<DateTime<Local>> {
    let day = day.parse::<u32>().ok()?;
    let month_num = match &month.to_ascii_lowercase()[..] {
        "jan" => 1,
        "feb" => 2,
        "mar" => 3,
        "apr" => 4,
        "may" => 5,
        "jun" => 6,
        "jul" => 7,
        "aug" => 8,
        "sep" => 9,
        "oct" => 10,
        "nov" => 11,
        "dec" => 12,
        _ => return None,
    };
    if let Some((hh, mm)) = time_or_year.split_once(':') {
        let year = Local::now().year();
        let date = NaiveDate::from_ymd_opt(year, month_num, day)?;
        let time = NaiveTime::from_hms_opt(hh.parse().ok()?, mm.parse().ok()?, 0)?;
        Local
            .from_local_datetime(&NaiveDateTime::new(date, time))
            .single()
    } else {
        let year = time_or_year.parse::<i32>().ok()?;
        let date = NaiveDate::from_ymd_opt(year, month_num, day)?;
        let time = NaiveTime::from_hms_opt(0, 0, 0)?;
        Local
            .from_local_datetime(&NaiveDateTime::new(date, time))
            .single()
    }
}

fn parse_mode_bits(txt: &str) -> u32 {
    let mut mode = 0u32;
    let chars: Vec<char> = txt.chars().collect();
    for (idx, ch) in chars.iter().enumerate().skip(1).take(9) {
        let bit = match idx {
            1 => 0o400,
            2 => 0o200,
            3 => 0o100,
            4 => 0o040,
            5 => 0o020,
            6 => 0o010,
            7 => 0o004,
            8 => 0o002,
            9 => 0o001,
            _ => 0,
        };
        if *ch != '-' {
            mode |= bit;
        }
    }
    mode
}

trait IfEmpty {
    fn if_empty(self, fallback: &str) -> String;
}

impl IfEmpty for &str {
    fn if_empty(self, fallback: &str) -> String {
        if self.is_empty() {
            fallback.to_string()
        } else {
            self.to_string()
        }
    }
}
