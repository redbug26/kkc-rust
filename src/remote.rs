use crate::config::project_dirs;
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use imap::{Client, Session};
use native_tls::{TlsConnector, TlsStream};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteSource {
    SshConfig,
    UserToml,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteProtocol {
    Sftp,
    Imap,
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
    Imap(ImapProfile),
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
pub struct ImapProfile {
    pub host: String,
    pub user: String,
    pub port: Option<u16>,
    pub path: Option<String>,
    pub password: Option<String>,
}

impl RemoteProfile {
    pub fn protocol(&self) -> RemoteProtocol {
        match self.kind {
            RemoteKind::Sftp(_) => RemoteProtocol::Sftp,
            RemoteKind::Imap(_) => RemoteProtocol::Imap,
        }
    }

    pub fn host_label(&self) -> String {
        match &self.kind {
            RemoteKind::Sftp(sftp) => sftp.host.clone().unwrap_or_else(|| self.name.clone()),
            RemoteKind::Imap(imap) => imap.host.clone(),
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
    imap: Vec<ImapProfileToml>,
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
struct ImapProfileToml {
    name: String,
    host: String,
    user: String,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    password: Option<String>,
}

type ImapSession = Session<TlsStream<TcpStream>>;

pub fn connections_path() -> Result<PathBuf> {
    let dirs = project_dirs()?;
    let dir = dirs.config_dir();
    fs::create_dir_all(dir)?;
    Ok(dir.join("connections.toml"))
}

pub fn load_profiles() -> Result<Vec<RemoteProfile>> {
    let mut out = load_ssh_profiles().unwrap_or_default();
    out.extend(load_saved_profiles()?);
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

pub fn save_profile(profile: &RemoteProfile) -> Result<()> {
    let path = connections_path()?;
    let mut store = if path.exists() {
        let text = fs::read_to_string(&path)
            .with_context(|| format!("Reading {}", path.display()))?;
        toml::from_str::<ConnectionStore>(&text)
            .with_context(|| format!("Parsing {}", path.display()))?
    } else {
        ConnectionStore::default()
    };

    match &profile.kind {
        RemoteKind::Sftp(sftp) => {
            store.sftp.retain(|p| !p.name.eq_ignore_ascii_case(&profile.name));
            store.sftp.push(SftpProfileToml {
                name: profile.name.clone(),
                host: sftp.host.clone(),
                user: sftp.user.clone(),
                port: sftp.port,
                path: sftp.path.clone(),
                identity_file: sftp.identity_file.clone(),
            });
        }
        RemoteKind::Imap(imap) => {
            store.imap.retain(|p| !p.name.eq_ignore_ascii_case(&profile.name));
            store.imap.push(ImapProfileToml {
                name: profile.name.clone(),
                host: imap.host.clone(),
                user: imap.user.clone(),
                port: imap.port,
                path: imap.path.clone(),
                password: imap.password.clone(),
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
        RemoteKind::Imap(imap) => Ok(imap.path.clone().unwrap_or_else(|| "/".into())),
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
        RemoteProtocol::Imap => {
            let trimmed = cwd.trim();
            if trimmed.is_empty() || trimmed == "/" {
                "/".into()
            } else {
                format!("/{}", trimmed.trim_matches('/'))
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
    progress(format!("Listing {}...", if cwd == "/" { "root" } else { &cwd }));
    let entries = match &profile.kind {
        RemoteKind::Sftp(_) => list_sftp_dir(profile, &cwd, show_hidden)?,
        RemoteKind::Imap(imap) => list_imap_dir_with_progress(imap, &cwd, progress)?,
    };
    if cancel.load(Ordering::Relaxed) {
        bail!("Aborted");
    }
    Ok((cwd, entries))
}

pub fn display_path(profile: &RemoteProfile, cwd: &str) -> String {
    match profile.protocol() {
        RemoteProtocol::Sftp => format!("sftp://{}{}", profile.name, cwd),
        RemoteProtocol::Imap => {
            if cwd == "/" {
                format!("imap://{}/", profile.name)
            } else {
                let mailbox = decode_mailbox_component(cwd.trim_start_matches('/'));
                format!("imap://{}/{}", profile.name, mailbox)
            }
        }
    }
}

pub fn list_dir(profile: &RemoteProfile, cwd: &str, show_hidden: bool) -> Result<Vec<RemoteEntry>> {
    match &profile.kind {
        RemoteKind::Sftp(_) => list_sftp_dir(profile, cwd, show_hidden),
        RemoteKind::Imap(imap) => list_imap_dir(imap, cwd),
    }
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
        RemoteKind::Imap(imap) => download_imap_into_dir(imap, remote_path, local_dir),
    }
}

pub fn download_bulk_into_dir(profile: &RemoteProfile, remote_path: &str, local_dir: &Path) -> Result<PathBuf> {
    match &profile.kind {
        RemoteKind::Sftp(_) => download_sftp_bulk_into_dir(profile, remote_path, local_dir),
        RemoteKind::Imap(imap) => download_imap_into_dir(imap, remote_path, local_dir),
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
        RemoteKind::Imap(_) => bail!("Upload to IMAP is not supported"),
    }
}

pub fn upload_bulk_into_dir(profile: &RemoteProfile, local_path: &Path, remote_dir: &str) -> Result<String> {
    match &profile.kind {
        RemoteKind::Sftp(_) => upload_sftp_bulk_into_dir(profile, local_path, remote_dir),
        RemoteKind::Imap(_) => bail!("Upload to IMAP is not supported"),
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
        RemoteKind::Imap(_) => bail!("Rename on IMAP is not supported"),
    }
}

pub fn make_dir(profile: &RemoteProfile, remote_path: &str) -> Result<()> {
    match &profile.kind {
        RemoteKind::Sftp(_) => {
            run_sftp_batch(profile, &[format!("mkdir {}", batch_quote(remote_path))])?;
            Ok(())
        }
        RemoteKind::Imap(_) => bail!("Create mailbox from KKC is not supported yet"),
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
        RemoteKind::Imap(_) => bail!("Delete on IMAP is not supported yet"),
    }
}

pub fn download_to_temp(profile: &RemoteProfile, remote_path: &str, recursive: bool) -> Result<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let base = std::env::temp_dir()
        .join("kkc-remote")
        .join(format!("{}-{}", std::process::id(), stamp));
    download_into_dir(profile, remote_path, &base, recursive)
}

#[allow(dead_code)]
pub fn remote_stats(profile: &RemoteProfile, remote_path: &str, is_dir: bool) -> Result<RemoteStats> {
    match &profile.kind {
        RemoteKind::Sftp(_) => remote_sftp_stats(profile, remote_path, is_dir),
        RemoteKind::Imap(imap) => remote_imap_stats(imap, remote_path, is_dir),
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
        RemoteKind::Imap(imap) => scan_imap_stats(imap, remote_path, is_dir, progress, cancel),
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
        RemoteKind::Sftp(_) => download_sftp_with_progress(profile, remote_path, local_dir, recursive, progress),
        RemoteKind::Imap(imap) => download_imap_with_progress(imap, remote_path, local_dir, progress),
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
        RemoteKind::Sftp(_) => upload_sftp_with_progress(profile, local_path, remote_dir, recursive, progress),
        RemoteKind::Imap(_) => bail!("Upload to IMAP is not supported"),
    }
}

fn load_saved_profiles() -> Result<Vec<RemoteProfile>> {
    let path = connections_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path)
        .with_context(|| format!("Reading {}", path.display()))?;
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
    out.extend(store.imap.into_iter().map(|p| RemoteProfile {
        name: p.name,
        source: RemoteSource::UserToml,
        kind: RemoteKind::Imap(ImapProfile {
            host: p.host,
            user: p.user,
            port: p.port,
            path: p.path,
            password: p.password,
        }),
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
        let Some(key) = parts.next() else { continue; };
        let rest = parts.collect::<Vec<_>>().join(" ");
        match key.to_ascii_lowercase().as_str() {
            "host" => {
                flush(&mut out, &mut current_hosts, &host_name, &user, port, &identity_file);
                host_name = None;
                user = None;
                port = None;
                identity_file = None;
                current_hosts = rest
                    .split_whitespace()
                    .filter(|alias| !alias.contains('*') && !alias.contains('?') && !alias.starts_with('!'))
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
    flush(&mut out, &mut current_hosts, &host_name, &user, port, &identity_file);
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

fn list_sftp_dir(profile: &RemoteProfile, cwd: &str, show_hidden: bool) -> Result<Vec<RemoteEntry>> {
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
    download_sftp_path::<fn(&str, u64) -> bool>(profile, remote_path, &local_target, recursive, None)?;
    Ok(local_target)
}

fn download_sftp_bulk_into_dir(profile: &RemoteProfile, remote_path: &str, local_dir: &Path) -> Result<PathBuf> {
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
    let name = local_path.file_name().context("local path has no file name")?;
    let remote_target = join_remote(remote_dir, &name.to_string_lossy());
    upload_sftp_path::<fn(&str, u64) -> bool>(profile, local_path, &remote_target, recursive, None)?;
    Ok(remote_target)
}

fn upload_sftp_bulk_into_dir(profile: &RemoteProfile, local_path: &Path, remote_dir: &str) -> Result<String> {
    let name = local_path.file_name().context("local path has no file name")?;
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

fn remote_sftp_stats(profile: &RemoteProfile, remote_path: &str, is_dir: bool) -> Result<RemoteStats> {
    if !is_dir {
        let parent = Path::new(remote_path).parent().unwrap_or(Path::new("/"));
        let file_name = Path::new(remote_path).file_name().unwrap_or_default().to_string_lossy().into_owned();
        let entries = list_sftp_dir(profile, &parent.to_string_lossy(), true)?;
        let size = entries
            .into_iter()
            .find(|e| e.name == file_name)
            .map(|e| e.size)
            .unwrap_or(0);
        return Ok(RemoteStats { files: 1, bytes: size });
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
    download_sftp_path(profile, remote_path, &local_target, recursive, Some(progress))?;
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
    let name = local_path.file_name().context("local path has no file name")?;
    let remote_target = join_remote(remote_dir, &name.to_string_lossy());
    upload_sftp_path(profile, local_path, &remote_target, recursive, Some(progress))?;
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

fn remote_sftp_dir_stats_recursive(profile: &RemoteProfile, remote_path: &str) -> Result<RemoteStats> {
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
            let delta = RemoteStats { files: 1, bytes: child.size };
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
            && !cb(remote_path, fs::metadata(local_target).map(|m| m.len()).unwrap_or(0))
        {
            bail!("Aborted");
        }
        return Ok(());
    }

    fs::create_dir_all(local_target)?;
    for child in list_sftp_dir(profile, remote_path, true)? {
        let child_local = local_target.join(&child.name);
        if child.is_dir {
            download_sftp_path(profile, &child.path, &child_local, true, progress.as_deref_mut())?;
        } else {
            download_sftp_path(profile, &child.path, &child_local, false, progress.as_deref_mut())?;
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
            && !cb(&local_path.to_string_lossy(), fs::metadata(local_path).map(|m| m.len()).unwrap_or(0))
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
            upload_sftp_path(profile, &child_local, &child_remote, true, progress.as_deref_mut())?;
        } else {
            upload_sftp_path(profile, &child_local, &child_remote, false, progress.as_deref_mut())?;
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
    if let Some(port) = sftp.port {
        cmd.arg("-P").arg(port.to_string());
    }
    if let Some(identity) = sftp.identity_file.as_ref().filter(|s| !s.trim().is_empty()) {
        cmd.arg("-i").arg(expand_tilde(identity));
    }
    cmd.arg(remote_target(profile));
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

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

fn list_imap_dir(profile: &ImapProfile, cwd: &str) -> Result<Vec<RemoteEntry>> {
    if cwd == "/" {
        return list_imap_mailboxes(profile, "/");
    }
    list_imap_mailbox_contents(profile, cwd)
}

fn list_imap_dir_with_progress<F>(profile: &ImapProfile, cwd: &str, progress: &mut F) -> Result<Vec<RemoteEntry>>
where
    F: FnMut(String),
{
    if cwd == "/" {
        return list_imap_mailboxes_with_progress(profile, "/", progress);
    }
    list_imap_mailbox_contents_with_progress(profile, cwd, progress)
}

fn remote_imap_stats(profile: &ImapProfile, remote_path: &str, is_dir: bool) -> Result<RemoteStats> {
    if !is_dir {
        let (_, uid) = parse_imap_message_path(remote_path)?;
        let message = fetch_imap_message_meta(profile, remote_path)?;
        return Ok(RemoteStats { files: usize::from(uid > 0), bytes: message.size });
    }
    let mailbox = decode_mailbox_path(remote_path)?;
    let messages = imap_fetch_messages(profile, &mailbox)?;
    Ok(RemoteStats {
        files: messages.len(),
        bytes: messages.iter().map(|m| m.size).sum(),
    })
}

fn scan_imap_stats<F>(
    profile: &ImapProfile,
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
        let stats = remote_imap_stats(profile, remote_path, false)?;
        progress(stats);
        return Ok(stats);
    }
    let mailbox = decode_mailbox_path(remote_path)?;
    let messages = imap_fetch_messages(profile, &mailbox)?;
    let mut total = RemoteStats::default();
    for msg in messages {
        if cancel.load(Ordering::Relaxed) {
            bail!("Aborted");
        }
        let delta = RemoteStats { files: 1, bytes: msg.size };
        total.files += 1;
        total.bytes += msg.size;
        progress(delta);
    }
    Ok(total)
}

fn download_imap_into_dir(profile: &ImapProfile, remote_path: &str, local_dir: &Path) -> Result<PathBuf> {
    if is_imap_message_path(remote_path) {
        return save_imap_message_to_dir(profile, remote_path, local_dir);
    }
    let mailbox = decode_mailbox_path(remote_path)?;
    let target = local_dir.join(safe_fs_name(&mailbox));
    fs::create_dir_all(&target)?;
    for message in imap_fetch_messages(profile, &mailbox)? {
        let message_path = format!("{}/{}", remote_path.trim_end_matches('/'), message.uid);
        let _ = save_imap_message_to_dir(profile, &message_path, &target)?;
    }
    Ok(target)
}

fn download_imap_with_progress<F>(
    profile: &ImapProfile,
    remote_path: &str,
    local_dir: &Path,
    progress: &mut F,
) -> Result<PathBuf>
where
    F: FnMut(&str, u64) -> bool,
{
    let path = save_imap_message_to_dir(profile, remote_path, local_dir)?;
    let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    if !progress(remote_path, size) {
        bail!("Aborted");
    }
    Ok(path)
}

fn list_imap_mailboxes(profile: &ImapProfile, parent_cwd: &str) -> Result<Vec<RemoteEntry>> {
    let mut noop = |_msg: &str| {};
    let mut session = imap_connect_with_progress(profile, &mut noop)?;
    let names = session.list(None, Some("*")).context("Listing IMAP mailboxes")?;
    let entries = build_imap_mailbox_entries(&names, parent_cwd);
    let _ = session.logout();
    Ok(entries)
}

fn list_imap_mailboxes_with_progress<F>(profile: &ImapProfile, parent_cwd: &str, progress: &mut F) -> Result<Vec<RemoteEntry>>
where
    F: FnMut(String),
{
    let mut phase = |msg: &str| progress(msg.to_string());
    let mut session = imap_connect_with_progress(profile, &mut phase)?;
    progress("Listing mailboxes...".into());
    let names = session.list(None, Some("*")).context("Listing IMAP mailboxes")?;
    let entries = build_imap_mailbox_entries(&names, parent_cwd);
    let _ = session.logout();
    Ok(entries)
}

fn list_imap_mailbox_contents(profile: &ImapProfile, cwd: &str) -> Result<Vec<RemoteEntry>> {
    let mut entries = list_imap_mailboxes(profile, cwd)?;
    entries.extend(list_imap_messages(profile, cwd)?);
    Ok(entries)
}

fn list_imap_messages(profile: &ImapProfile, cwd: &str) -> Result<Vec<RemoteEntry>> {
    let mailbox = decode_mailbox_path(cwd)?;
    let mut messages = imap_fetch_messages(profile, &mailbox)?;
    messages.sort_by(|a, b| b.uid.cmp(&a.uid));
    Ok(messages
        .into_iter()
        .map(|msg| {
            let subject = msg.subject.unwrap_or_else(|| "message".into());
            let display = format!("{:08} {}", msg.uid, safe_mail_subject(&subject));
            RemoteEntry {
                name: truncate_name(&display, 120),
                path: format!("{}/{}", cwd.trim_end_matches('/'), msg.uid),
                is_dir: false,
                is_symlink: false,
                size: msg.size,
                modified: msg.modified,
                mode: 0o644,
            }
        })
        .collect())
}

fn list_imap_mailbox_contents_with_progress<F>(profile: &ImapProfile, cwd: &str, progress: &mut F) -> Result<Vec<RemoteEntry>>
where
    F: FnMut(String),
{
    let mut entries = list_imap_mailboxes_with_progress(profile, cwd, progress)?;
    entries.extend(list_imap_messages_with_progress(profile, cwd, progress)?);
    Ok(entries)
}

fn list_imap_messages_with_progress<F>(profile: &ImapProfile, cwd: &str, progress: &mut F) -> Result<Vec<RemoteEntry>>
where
    F: FnMut(String),
{
    let mailbox = decode_mailbox_path(cwd)?;
    let mut messages = imap_fetch_messages_with_progress(profile, &mailbox, progress)?;
    messages.sort_by(|a, b| b.uid.cmp(&a.uid));
    Ok(messages
        .into_iter()
        .map(|msg| {
            let subject = msg.subject.unwrap_or_else(|| "message".into());
            let display = format!("{:08} {}", msg.uid, safe_mail_subject(&subject));
            RemoteEntry {
                name: truncate_name(&display, 120),
                path: format!("{}/{}", cwd.trim_end_matches('/'), msg.uid),
                is_dir: false,
                is_symlink: false,
                size: msg.size,
                modified: msg.modified,
                mode: 0o644,
            }
        })
        .collect())
}

fn build_imap_mailbox_entries(names: &[imap::types::Name], parent_cwd: &str) -> Vec<RemoteEntry> {
    use imap::types::NameAttribute;
    use std::collections::HashSet;

    let parent_mailbox = if parent_cwd == "/" {
        None
    } else {
        decode_mailbox_path(parent_cwd).ok()
    };
    let mut seen = HashSet::new();
    let mut entries = Vec::new();

    for name in names {
        let mailbox = name.name();
        let delimiter = name.delimiter().unwrap_or("/");
        let child = match parent_mailbox.as_deref() {
            None => {
                if let Some((head, _)) = mailbox.split_once(delimiter) {
                    head.to_string()
                } else {
                    mailbox.to_string()
                }
            }
            Some(parent) => {
                let Some(rest) = mailbox.strip_prefix(parent) else {
                    continue;
                };
                let Some(rest) = rest.strip_prefix(delimiter) else {
                    continue;
                };
                if rest.is_empty() {
                    continue;
                }
                if let Some((head, _)) = rest.split_once(delimiter) {
                    format!("{parent}{delimiter}{head}")
                } else {
                    mailbox.to_string()
                }
            }
        };

        if !seen.insert(child.clone()) {
            continue;
        }
        let display_name = match parent_mailbox.as_deref() {
            None => {
                if let Some((head, _)) = child.split_once(delimiter) {
                    head.to_string()
                } else {
                    child.clone()
                }
            }
            Some(parent) => child
                .strip_prefix(parent)
                .and_then(|s| s.strip_prefix(delimiter))
                .unwrap_or(&child)
                .to_string(),
        };
        let is_noselect = name
            .attributes()
            .iter()
            .any(|attr| matches!(attr, NameAttribute::NoSelect));
        entries.push(RemoteEntry {
            name: display_name,
            path: format!("/{}", encode_mailbox_component(&child)),
            is_dir: true,
            is_symlink: false,
            size: 0,
            modified: None,
            mode: if is_noselect { 0o555 } else { 0o755 },
        });
    }

    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    entries
}

fn fetch_imap_message_meta(profile: &ImapProfile, remote_path: &str) -> Result<ImapMessageMeta> {
    let (mailbox, uid) = parse_imap_message_path(remote_path)?;
    let mut session = imap_connect(profile)?;
    session.select(&mailbox)?;
    let fetches = session
        .uid_fetch(uid.to_string(), "(UID RFC822.SIZE INTERNALDATE BODY.PEEK[HEADER.FIELDS (SUBJECT)])")
        .context("Fetching IMAP message metadata")?;
    let Some(fetch) = fetches.iter().next() else {
        bail!("Message not found");
    };
    let meta = parse_imap_fetch(fetch)?;
    let _ = session.logout();
    Ok(meta)
}

fn save_imap_message_to_dir(profile: &ImapProfile, remote_path: &str, local_dir: &Path) -> Result<PathBuf> {
    let (mailbox, uid) = parse_imap_message_path(remote_path)?;
    let mut session = imap_connect(profile)?;
    session.select(&mailbox)?;
    let body = fetch_imap_message_bytes(&mut session, uid)
        .with_context(|| format!("Downloading IMAP message UID {} from {}", uid, mailbox))?;
    fs::create_dir_all(local_dir)?;
    let target = local_dir.join(format!("{uid:08}.eml"));
    fs::write(&target, body)?;
    let _ = session.logout();
    Ok(target)
}

fn fetch_imap_message_bytes(session: &mut ImapSession, uid: u32) -> Result<Vec<u8>> {
    for query in ["BODY.PEEK[]", "BODY[]", "RFC822"] {
        let fetches = session
            .uid_fetch(uid.to_string(), query)
            .with_context(|| format!("IMAP FETCH {}", query))?;
        if let Some(fetch) = fetches.iter().next()
            && let Some(body) = fetch.body()
        {
            return Ok(body.to_vec());
        }
    }
    bail!("Missing IMAP message body")
}

fn imap_fetch_messages(profile: &ImapProfile, mailbox: &str) -> Result<Vec<ImapMessageMeta>> {
    let mut noop = |_msg: &str| {};
    let mut session = imap_connect_with_progress(profile, &mut noop)?;
    session.select(mailbox)?;
    let fetches = session
        .fetch("1:*", "(UID RFC822.SIZE INTERNALDATE BODY.PEEK[HEADER.FIELDS (SUBJECT)])")
        .context("Listing IMAP messages")?;
    let mut out = Vec::new();
    for fetch in fetches.iter() {
        if let Ok(meta) = parse_imap_fetch(fetch) {
            out.push(meta);
        }
    }
    let _ = session.logout();
    Ok(out)
}

fn imap_fetch_messages_with_progress<F>(profile: &ImapProfile, mailbox: &str, progress: &mut F) -> Result<Vec<ImapMessageMeta>>
where
    F: FnMut(String),
{
    let mut phase = |msg: &str| progress(msg.to_string());
    let mut session = imap_connect_with_progress(profile, &mut phase)?;
    progress(format!("Selecting mailbox {}...", mailbox));
    session.select(mailbox)?;
    progress("Fetching message headers...".into());
    let fetches = session
        .fetch("1:*", "(UID RFC822.SIZE INTERNALDATE BODY.PEEK[HEADER.FIELDS (SUBJECT)])")
        .context("Listing IMAP messages")?;
    let mut out = Vec::new();
    for fetch in fetches.iter() {
        if let Ok(meta) = parse_imap_fetch(fetch) {
            out.push(meta);
        }
    }
    let _ = session.logout();
    Ok(out)
}

#[derive(Debug)]
struct ImapMessageMeta {
    uid: u32,
    size: u64,
    modified: Option<DateTime<Local>>,
    subject: Option<String>,
}

fn parse_imap_fetch(fetch: &imap::types::Fetch) -> Result<ImapMessageMeta> {
    let uid = fetch.uid.ok_or_else(|| anyhow::anyhow!("Missing IMAP UID"))?;
    let size = fetch.size.unwrap_or(0) as u64;
    let modified = fetch
        .internal_date()
        .and_then(|date| {
            let ts = date.timestamp();
            Local.timestamp_opt(ts, 0).single()
        });
    let subject = fetch
        .header()
        .map(|raw| parse_header_value(raw, "subject"))
        .transpose()?
        .flatten();
    Ok(ImapMessageMeta {
        uid,
        size,
        modified,
        subject,
    })
}

fn parse_header_value(raw: &[u8], key: &str) -> Result<Option<String>> {
    let text = String::from_utf8_lossy(raw);
    for line in text.lines() {
        if let Some((head, value)) = line.split_once(':')
            && head.trim().eq_ignore_ascii_case(key)
        {
            return Ok(Some(value.trim().to_string()));
        }
    }
    Ok(None)
}

fn imap_connect(profile: &ImapProfile) -> Result<ImapSession> {
    let mut noop = |_msg: &str| {};
    imap_connect_with_progress(profile, &mut noop)
}

fn imap_connect_with_progress<F>(profile: &ImapProfile, progress: &mut F) -> Result<ImapSession>
where
    F: FnMut(&str),
{
    let tls = TlsConnector::builder().build().context("Building TLS connector")?;
    let host = profile.host.trim();
    if host.is_empty() {
        bail!("IMAP host is required");
    }
    let port = profile.port.unwrap_or(993);
    progress("Resolving host");
    let addr = (host, port)
        .to_socket_addrs()
        .context("Resolving IMAP host")?
        .next()
        .context("No IMAP socket address resolved")?;
    progress("Opening TCP socket");
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(10))
        .context("Connecting to IMAP")?;
    stream.set_read_timeout(Some(Duration::from_secs(15))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(15))).ok();
    progress("Negotiating TLS");
    let tls_stream = Client::new(
        TlsConnector::connect(&tls, host, stream).map_err(|err| anyhow::anyhow!("TLS handshake failed: {err}"))?
    );
    let mut client = tls_stream;
    progress("Reading IMAP greeting");
    client.read_greeting().context("Reading IMAP greeting")?;
    let password = profile.password.clone().unwrap_or_default();
    progress("Authenticating");
    client
        .login(profile.user.as_str(), password)
        .map_err(|(err, _)| anyhow::anyhow!("IMAP login failed: {err}"))
}

fn parse_imap_message_path(path: &str) -> Result<(String, u32)> {
    let trimmed = path.trim_matches('/');
    let Some((mailbox_enc, uid_txt)) = trimmed.rsplit_once('/') else {
        bail!("Invalid IMAP message path");
    };
    let uid = uid_txt.parse::<u32>().context("Invalid IMAP UID")?;
    Ok((decode_mailbox_component(mailbox_enc), uid))
}

fn is_imap_message_path(path: &str) -> bool {
    path.trim_matches('/')
        .rsplit_once('/')
        .map(|(_, tail)| tail.parse::<u32>().is_ok())
        .unwrap_or(false)
}

fn decode_mailbox_path(path: &str) -> Result<String> {
    if path == "/" {
        bail!("Root is not a mailbox");
    }
    Ok(decode_mailbox_component(path.trim_matches('/')))
}

fn encode_mailbox_component(name: &str) -> String {
    name.replace('%', "%25").replace('/', "%2F")
}

fn decode_mailbox_component(name: &str) -> String {
    let mut out = String::new();
    let bytes = name.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            match &name[i..i + 3] {
                "%2F" => {
                    out.push('/');
                    i += 3;
                    continue;
                }
                "%25" => {
                    out.push('%');
                    i += 3;
                    continue;
                }
                _ => {}
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn safe_fs_name(name: &str) -> String {
    let cleaned = name
        .chars()
        .map(|ch| if matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { ch })
        .collect::<String>();
    if cleaned.is_empty() { "mailbox".into() } else { cleaned }
}

fn safe_mail_subject(subject: &str) -> String {
    let subject = subject.replace('\r', " ").replace('\n', " ");
    let collapsed = subject.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        "message.eml".into()
    } else {
        format!("{}.eml", collapsed)
    }
}

fn truncate_name(name: &str, max: usize) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if out.chars().count() >= max {
            break;
        }
        out.push(ch);
    }
    out
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
    if line.is_empty() || line.starts_with("sftp>") || line.starts_with("Connected to") || line.starts_with("Fetching") || line.starts_with("Remote working directory:") || line.starts_with("total ") {
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
        "jan" => 1, "feb" => 2, "mar" => 3, "apr" => 4, "may" => 5, "jun" => 6,
        "jul" => 7, "aug" => 8, "sep" => 9, "oct" => 10, "nov" => 11, "dec" => 12,
        _ => return None,
    };
    if let Some((hh, mm)) = time_or_year.split_once(':') {
        let year = Local::now().year();
        let date = NaiveDate::from_ymd_opt(year, month_num, day)?;
        let time = NaiveTime::from_hms_opt(hh.parse().ok()?, mm.parse().ok()?, 0)?;
        Local.from_local_datetime(&NaiveDateTime::new(date, time)).single()
    } else {
        let year = time_or_year.parse::<i32>().ok()?;
        let date = NaiveDate::from_ymd_opt(year, month_num, day)?;
        let time = NaiveTime::from_hms_opt(0, 0, 0)?;
        Local.from_local_datetime(&NaiveDateTime::new(date, time)).single()
    }
}

fn parse_mode_bits(txt: &str) -> u32 {
    let mut mode = 0u32;
    let chars: Vec<char> = txt.chars().collect();
    for (idx, ch) in chars.iter().enumerate().skip(1).take(9) {
        let bit = match idx {
            1 => 0o400, 2 => 0o200, 3 => 0o100,
            4 => 0o040, 5 => 0o020, 6 => 0o010,
            7 => 0o004, 8 => 0o002, 9 => 0o001,
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
