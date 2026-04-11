use crate::config::project_dirs;
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteSource {
    SshConfig,
    UserToml,
}

#[derive(Debug, Clone)]
pub struct SftpProfile {
    pub name: String,
    pub host: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub path: Option<String>,
    pub identity_file: Option<String>,
    pub source: RemoteSource,
}

#[derive(Debug, Clone)]
pub struct RemoteEntry {
    pub name: String,
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

pub fn connections_path() -> Result<PathBuf> {
    let dirs = project_dirs()?;
    let dir = dirs.config_dir();
    fs::create_dir_all(dir)?;
    Ok(dir.join("connections.toml"))
}

pub fn load_profiles() -> Result<Vec<SftpProfile>> {
    let mut out = load_ssh_profiles().unwrap_or_default();
    out.extend(load_saved_profiles()?);
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

pub fn save_profile(profile: &SftpProfile) -> Result<()> {
    let path = connections_path()?;
    let mut store = if path.exists() {
        let text = fs::read_to_string(&path)
            .with_context(|| format!("Reading {}", path.display()))?;
        toml::from_str::<ConnectionStore>(&text)
            .with_context(|| format!("Parsing {}", path.display()))?
    } else {
        ConnectionStore::default()
    };

    store.sftp.retain(|p| !p.name.eq_ignore_ascii_case(&profile.name));
    store.sftp.push(SftpProfileToml {
        name: profile.name.clone(),
        host: profile.host.clone(),
        user: profile.user.clone(),
        port: profile.port,
        path: profile.path.clone(),
        identity_file: profile.identity_file.clone(),
    });

    let text = toml::to_string_pretty(&store).context("Serialising connections")?;
    fs::write(&path, text).with_context(|| format!("Writing {}", path.display()))?;
    Ok(())
}

pub fn resolve_initial_dir(profile: &SftpProfile) -> Result<String> {
    if let Some(path) = profile.path.as_ref().filter(|s| !s.trim().is_empty()) {
        return Ok(path.clone());
    }
    let out = run_batch(profile, &["pwd".into()])?;
    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("Remote working directory: ") {
            return Ok(rest.trim().to_string());
        }
    }
    Ok("/".into())
}

pub fn list_dir(profile: &SftpProfile, cwd: &str, show_hidden: bool) -> Result<Vec<RemoteEntry>> {
    let out = run_batch(
        profile,
        &[format!("cd {}", batch_quote(cwd)), "ls -la".into()],
    )?;
    let mut entries = Vec::new();
    for line in out.lines() {
        if let Some(entry) = parse_ls_line(line) {
            if entry.name == "." || entry.name == ".." {
                continue;
            }
            if !show_hidden && entry.name.starts_with('.') {
                continue;
            }
            entries.push(entry);
        }
    }
    Ok(entries)
}

pub fn join_remote(parent: &str, name: &str) -> String {
    if parent == "/" {
        format!("/{}", name)
    } else {
        format!("{}/{}", parent.trim_end_matches('/'), name)
    }
}

pub fn download_into_dir(
    profile: &SftpProfile,
    remote_path: &str,
    local_dir: &Path,
    recursive: bool,
) -> Result<PathBuf> {
    let name = Path::new(remote_path)
        .file_name()
        .context("remote path has no file name")?;
    let local_target = local_dir.join(name);
    download_path::<fn(&str, u64) -> bool>(profile, remote_path, &local_target, recursive, None)?;
    Ok(local_target)
}

pub fn download_bulk_into_dir(profile: &SftpProfile, remote_path: &str, local_dir: &Path) -> Result<PathBuf> {
    let name = Path::new(remote_path)
        .file_name()
        .context("remote path has no file name")?;
    let local_target = local_dir.join(name);
    if let Some(parent) = local_target.parent() {
        fs::create_dir_all(parent)?;
    }
    run_batch(
        profile,
        &[format!(
            "get -r {} {}",
            batch_quote(remote_path),
            batch_quote(&local_target.to_string_lossy())
        )],
    )?;
    Ok(local_target)
}

pub fn upload_into_dir(
    profile: &SftpProfile,
    local_path: &Path,
    remote_dir: &str,
    recursive: bool,
) -> Result<String> {
    let name = local_path.file_name().context("local path has no file name")?;
    let remote_target = join_remote(remote_dir, &name.to_string_lossy());
    upload_path::<fn(&str, u64) -> bool>(profile, local_path, &remote_target, recursive, None)?;
    Ok(remote_target)
}

pub fn upload_bulk_into_dir(profile: &SftpProfile, local_path: &Path, remote_dir: &str) -> Result<String> {
    let name = local_path.file_name().context("local path has no file name")?;
    let remote_target = join_remote(remote_dir, &name.to_string_lossy());
    run_batch(
        profile,
        &[format!(
            "put -r {} {}",
            batch_quote(&local_path.to_string_lossy()),
            batch_quote(&remote_target)
        )],
    )?;
    Ok(remote_target)
}

pub fn rename_path(profile: &SftpProfile, old_path: &str, new_path: &str) -> Result<()> {
    run_batch(
        profile,
        &[format!(
            "rename {} {}",
            batch_quote(old_path),
            batch_quote(new_path)
        )],
    )?;
    Ok(())
}

pub fn make_dir(profile: &SftpProfile, remote_path: &str) -> Result<()> {
    run_batch(profile, &[format!("mkdir {}", batch_quote(remote_path))])?;
    Ok(())
}

pub fn delete_path(profile: &SftpProfile, remote_path: &str, is_dir: bool) -> Result<()> {
    if is_dir {
        delete_dir_recursive(profile, remote_path)?;
    } else {
        run_batch(profile, &[format!("rm {}", batch_quote(remote_path))])?;
    }
    Ok(())
}

pub fn download_to_temp(profile: &SftpProfile, remote_path: &str, recursive: bool) -> Result<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let base = std::env::temp_dir()
        .join("kkc-remote")
        .join(format!("{}-{}", std::process::id(), stamp));
    download_into_dir(profile, remote_path, &base, recursive)
}

pub fn remote_stats(profile: &SftpProfile, remote_path: &str, is_dir: bool) -> Result<RemoteStats> {
    if !is_dir {
        let parent = Path::new(remote_path).parent().unwrap_or(Path::new("/"));
        let file_name = Path::new(remote_path).file_name().unwrap_or_default().to_string_lossy().into_owned();
        let entries = list_dir(profile, &parent.to_string_lossy(), true)?;
        let size = entries
            .into_iter()
            .find(|e| e.name == file_name)
            .map(|e| e.size)
            .unwrap_or(0);
        return Ok(RemoteStats { files: 1, bytes: size });
    }
    remote_dir_stats_recursive(profile, remote_path)
}

pub fn scan_remote_stats<F>(
    profile: &SftpProfile,
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
        let stats = remote_stats(profile, remote_path, false)?;
        progress(stats);
        return Ok(stats);
    }
    scan_remote_dir_recursive(profile, remote_path, progress, cancel)
}

pub fn download_with_progress<F>(
    profile: &SftpProfile,
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
    download_path(profile, remote_path, &local_target, recursive, Some(progress))?;
    Ok(local_target)
}

pub fn upload_with_progress<F>(
    profile: &SftpProfile,
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
    upload_path(profile, local_path, &remote_target, recursive, Some(progress))?;
    Ok(remote_target)
}

fn load_saved_profiles() -> Result<Vec<SftpProfile>> {
    let path = connections_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path)
        .with_context(|| format!("Reading {}", path.display()))?;
    let store = toml::from_str::<ConnectionStore>(&text)
        .with_context(|| format!("Parsing {}", path.display()))?;
    Ok(store
        .sftp
        .into_iter()
        .map(|p| SftpProfile {
            name: p.name,
            host: p.host,
            user: p.user,
            port: p.port,
            path: p.path,
            identity_file: p.identity_file,
            source: RemoteSource::UserToml,
        })
        .collect())
}

fn load_ssh_profiles() -> Result<Vec<SftpProfile>> {
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

    let flush = |out: &mut Vec<SftpProfile>,
                 current_hosts: &mut Vec<String>,
                 host_name: &Option<String>,
                 user: &Option<String>,
                 port: Option<u16>,
                 identity_file: &Option<String>| {
        for alias in current_hosts.drain(..) {
            out.push(SftpProfile {
                name: alias,
                host: host_name.clone(),
                user: user.clone(),
                port,
                path: None,
                identity_file: identity_file.clone(),
                source: RemoteSource::SshConfig,
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

fn delete_dir_recursive(profile: &SftpProfile, remote_path: &str) -> Result<()> {
    let children = list_dir(profile, remote_path, true)?;
    for child in children {
        let child_path = join_remote(remote_path, &child.name);
        if child.is_dir {
            delete_dir_recursive(profile, &child_path)?;
        } else {
            run_batch(profile, &[format!("rm {}", batch_quote(&child_path))])?;
        }
    }
    run_batch(profile, &[format!("rmdir {}", batch_quote(remote_path))])?;
    Ok(())
}

fn remote_dir_stats_recursive(profile: &SftpProfile, remote_path: &str) -> Result<RemoteStats> {
    let mut stats = RemoteStats::default();
    for child in list_dir(profile, remote_path, true)? {
        let child_path = join_remote(remote_path, &child.name);
        if child.is_dir {
            let sub = remote_dir_stats_recursive(profile, &child_path)?;
            stats.files += sub.files;
            stats.bytes += sub.bytes;
        } else {
            stats.files += 1;
            stats.bytes += child.size;
        }
    }
    Ok(stats)
}

fn scan_remote_dir_recursive<F>(
    profile: &SftpProfile,
    remote_path: &str,
    progress: &mut F,
    cancel: &Arc<AtomicBool>,
) -> Result<RemoteStats>
where
    F: FnMut(RemoteStats),
{
    let mut stats = RemoteStats::default();
    for child in list_dir(profile, remote_path, true)? {
        if cancel.load(Ordering::Relaxed) {
            bail!("Aborted");
        }
        let child_path = join_remote(remote_path, &child.name);
        if child.is_dir {
            let sub = scan_remote_dir_recursive(profile, &child_path, progress, cancel)?;
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

fn download_path<F>(
    profile: &SftpProfile,
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
        run_batch(
            profile,
            &[format!(
                "get {} {}",
                batch_quote(remote_path),
                batch_quote(&local_target.to_string_lossy())
            )],
        )?;
        if let Some(cb) = progress.as_mut() {
            if !cb(remote_path, fs::metadata(local_target).map(|m| m.len()).unwrap_or(0)) {
                bail!("Aborted");
            }
        }
        return Ok(());
    }

    fs::create_dir_all(local_target)?;
    for child in list_dir(profile, remote_path, true)? {
        let child_remote = join_remote(remote_path, &child.name);
        let child_local = local_target.join(&child.name);
        if child.is_dir {
            download_path(profile, &child_remote, &child_local, true, progress.as_deref_mut())?;
        } else {
            download_path(profile, &child_remote, &child_local, false, progress.as_deref_mut())?;
        }
    }
    Ok(())
}

fn upload_path<F>(
    profile: &SftpProfile,
    local_path: &Path,
    remote_target: &str,
    recursive: bool,
    mut progress: Option<&mut F>,
) -> Result<()>
where
    F: FnMut(&str, u64) -> bool,
{
    if !recursive || !local_path.is_dir() {
        run_batch(
            profile,
            &[format!(
                "put {} {}",
                batch_quote(&local_path.to_string_lossy()),
                batch_quote(remote_target)
            )],
        )?;
        if let Some(cb) = progress.as_mut() {
            if !cb(&local_path.to_string_lossy(), fs::metadata(local_path).map(|m| m.len()).unwrap_or(0)) {
                bail!("Aborted");
            }
        }
        return Ok(());
    }

    let _ = run_batch(profile, &[format!("mkdir {}", batch_quote(remote_target))]);
    for entry in fs::read_dir(local_path)? {
        let entry = entry?;
        let child_local = entry.path();
        let child_remote = join_remote(remote_target, &entry.file_name().to_string_lossy());
        if child_local.is_dir() {
            upload_path(profile, &child_local, &child_remote, true, progress.as_deref_mut())?;
        } else {
            upload_path(profile, &child_local, &child_remote, false, progress.as_deref_mut())?;
        }
    }
    Ok(())
}

fn run_batch(profile: &SftpProfile, commands: &[String]) -> Result<String> {
    let mut cmd = Command::new("sftp");
    cmd.arg("-q").arg("-b").arg("-");
    if let Some(port) = profile.port {
        cmd.arg("-P").arg(port.to_string());
    }
    if let Some(identity) = profile.identity_file.as_ref().filter(|s| !s.trim().is_empty()) {
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

fn remote_target(profile: &SftpProfile) -> String {
    match (&profile.user, &profile.host) {
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
    let mut out = 0u32;
    let chars = txt.chars().collect::<Vec<_>>();
    let perms = [(1usize, 0o400), (2, 0o200), (3, 0o100), (4, 0o040), (5, 0o020), (6, 0o010), (7, 0o004), (8, 0o002), (9, 0o001)];
    for (idx, bit) in perms {
        if chars.get(idx).copied().unwrap_or('-') != '-' {
            out |= bit;
        }
    }
    out
}

trait IfEmpty {
    fn if_empty(self, fallback: &str) -> String;
}

impl IfEmpty for &str {
    fn if_empty(self, fallback: &str) -> String {
        if self.is_empty() { fallback.to_string() } else { self.to_string() }
    }
}
