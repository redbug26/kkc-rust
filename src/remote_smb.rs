//! SMB/Samba remote protocol implementation.
//!
//! This module is compiled only when the `smb` Cargo feature is enabled.
//! It is declared in `remote.rs` with:
//!   `#[cfg(feature = "smb")] #[path = "remote_smb.rs"] mod smb_impl;`

use super::{RemoteEntry, RemoteKind, RemoteProfile, RemoteStats, SmbProfile, join_remote};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Local, TimeZone};
use pavao::{SmbClient, SmbCredentials, SmbDirentType, SmbMode, SmbOpenOptions, SmbOptions};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// TOML persistence type (private to the remote module)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SmbProfileToml {
    pub(super) name: String,
    pub(super) host: String,
    #[serde(default)]
    pub(super) user: Option<String>,
    #[serde(default)]
    pub(super) password: Option<String>,
    #[serde(default)]
    pub(super) workgroup: Option<String>,
    #[serde(default)]
    pub(super) share: Option<String>,
    #[serde(default)]
    pub(super) path: Option<String>,
}

// ---------------------------------------------------------------------------
// Thin wrappers called from the dispatch functions in remote.rs
// ---------------------------------------------------------------------------

pub(super) fn smb_rename(smb: &SmbProfile, old_path: &str, new_path: &str) -> Result<()> {
    let client = smb_client(smb)?;
    client
        .rename(old_path, new_path)
        .map_err(|e| anyhow::anyhow!("SMB rename error: {e}"))
}

pub(super) fn smb_mkdir(smb: &SmbProfile, path: &str) -> Result<()> {
    let client = smb_client(smb)?;
    client
        .mkdir(path, SmbMode::from(0o755 as libc::mode_t))
        .map_err(|e| anyhow::anyhow!("SMB mkdir error: {e}"))
}

pub(super) fn smb_delete_file(smb: &SmbProfile, path: &str) -> Result<()> {
    let client = smb_client(smb)?;
    client
        .unlink(path)
        .map_err(|e| anyhow::anyhow!("SMB unlink error: {e}"))
}

// ---------------------------------------------------------------------------
// Functions called from the dispatch functions in remote.rs (pub(super))
// ---------------------------------------------------------------------------

/// Enumerate the available shares on an SMB server (no share required).
pub(super) fn list_smb_shares(profile: &RemoteProfile) -> Result<Vec<String>> {
    let RemoteKind::Smb(smb) = &profile.kind else {
        bail!("Profile is not SMB");
    };
    // Build a server-root connection WITHOUT one_share_per_server — that flag
    // is only appropriate when a specific share is selected; using it at the
    // server root suppresses the share listing on most Samba/NAS servers.
    let server_url = format!("smb://{}", smb.host);
    let mut creds = SmbCredentials::default().server(&server_url);
    if let Some(user) = smb.user.as_deref().filter(|s| !s.trim().is_empty()) {
        creds = creds.username(user);
    }
    if let Some(password) = smb.password.as_deref().filter(|s| !s.trim().is_empty()) {
        creds = creds.password(password);
    }
    if let Some(workgroup) = smb.workgroup.as_deref().filter(|s| !s.trim().is_empty()) {
        creds = creds.workgroup(workgroup);
    }
    let client = SmbClient::new(creds, SmbOptions::default())
        .map_err(|e| anyhow::anyhow!("SMB connect to {}: {}", server_url, e))?;
    // Use list_dir (not list_dirplus) — list_dir uses smbc_readdir which returns
    // proper SMB entity types (FileShare, IpcShare, etc.) at the server root.
    // list_dirplus uses smbc_readdirplus which retrieves file stats and does not
    // work reliably for share enumeration.
    let dirents = client
        .list_dir("")
        .or_else(|_| client.list_dir("/"))
        .map_err(|e| anyhow::anyhow!("SMB share enumeration on '{}': {}", smb.host, e))?;
    let mut shares: Vec<String> = dirents
        .into_iter()
        .filter(|d| matches!(d.get_type(), SmbDirentType::FileShare | SmbDirentType::Dir))
        .map(|d| d.name().to_owned())
        .collect();
    shares.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    if shares.is_empty() {
        bail!("Server responded but listed no shares (check credentials/permissions)");
    }
    Ok(shares)
}

pub(super) fn list_smb_dir(
    profile: &RemoteProfile,
    cwd: &str,
    show_hidden: bool,
) -> Result<Vec<RemoteEntry>> {
    let RemoteKind::Smb(smb) = &profile.kind else {
        bail!("Profile is not SMB");
    };
    let client = smb_client(smb)?;
    let smb_path = if cwd == "/" { "" } else { cwd };
    let entries = client.list_dirplus(smb_path).map_err(|e| {
        anyhow::anyhow!(
            "SMB list error listing '{}': {} (check credentials and share name)",
            smb_path,
            e
        )
    })?;
    let mut out = Vec::new();
    for dirent in entries {
        let name = dirent.name.clone();
        if name == "." || name == ".." {
            continue;
        }
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        let dtype = dirent.get_type();
        let is_dir = matches!(dtype, SmbDirentType::Dir | SmbDirentType::FileShare);
        let is_symlink = matches!(dtype, SmbDirentType::Link);
        let modified = systemtime_to_local(dirent.mtime);
        out.push(RemoteEntry {
            path: join_remote(cwd, &name),
            name,
            is_dir,
            is_symlink,
            size: dirent.size,
            modified,
            mode: if is_dir { 0o755 } else { 0o644 },
        });
    }
    Ok(out)
}

pub(super) fn download_smb_into_dir(
    profile: &RemoteProfile,
    remote_path: &str,
    local_dir: &Path,
    recursive: bool,
) -> Result<PathBuf> {
    let name = Path::new(remote_path)
        .file_name()
        .context("remote path has no file name")?;
    let local_target = local_dir.join(name);
    download_smb_path::<fn(&str, u64) -> bool>(
        profile,
        remote_path,
        &local_target,
        recursive,
        None,
    )?;
    Ok(local_target)
}

pub(super) fn upload_smb_into_dir(
    profile: &RemoteProfile,
    local_path: &Path,
    remote_dir: &str,
    recursive: bool,
) -> Result<String> {
    let name = local_path
        .file_name()
        .context("local path has no file name")?;
    let remote_target = join_remote(remote_dir, &name.to_string_lossy());
    upload_smb_path::<fn(&str, u64) -> bool>(profile, local_path, &remote_target, recursive, None)?;
    Ok(remote_target)
}

pub(super) fn download_smb_with_progress<F>(
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
    download_smb_path(
        profile,
        remote_path,
        &local_target,
        recursive,
        Some(progress),
    )?;
    Ok(local_target)
}

pub(super) fn upload_smb_with_progress<F>(
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
    upload_smb_path(
        profile,
        local_path,
        &remote_target,
        recursive,
        Some(progress),
    )?;
    Ok(remote_target)
}

pub(super) fn delete_smb_dir_recursive(profile: &RemoteProfile, remote_path: &str) -> Result<()> {
    let RemoteKind::Smb(smb) = &profile.kind else {
        bail!("Profile is not SMB");
    };
    let children = list_smb_dir(profile, remote_path, true)?;
    for child in children {
        if child.is_dir {
            delete_smb_dir_recursive(profile, &child.path)?;
        } else {
            let client = smb_client(smb)?;
            client
                .unlink(&child.path)
                .map_err(|e| anyhow::anyhow!("SMB unlink error: {e}"))?;
        }
    }
    let client = smb_client(smb)?;
    client
        .rmdir(remote_path)
        .map_err(|e| anyhow::anyhow!("SMB rmdir error: {e}"))
}

pub(super) fn remote_smb_stats(
    profile: &RemoteProfile,
    remote_path: &str,
    is_dir: bool,
) -> Result<RemoteStats> {
    if !is_dir {
        let RemoteKind::Smb(smb) = &profile.kind else {
            bail!("Profile is not SMB");
        };
        let client = smb_client(smb)?;
        let stat = client
            .stat(remote_path)
            .map_err(|e| anyhow::anyhow!("SMB stat error: {e}"))?;
        return Ok(RemoteStats {
            files: 1,
            bytes: stat.size,
        });
    }
    remote_smb_dir_stats_recursive(profile, remote_path)
}

pub(super) fn scan_smb_stats<F>(
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
        let stats = remote_smb_stats(profile, remote_path, false)?;
        progress(stats);
        return Ok(stats);
    }
    scan_smb_dir_recursive(profile, remote_path, progress, cancel)
}

// ---------------------------------------------------------------------------
// Private implementation helpers
// ---------------------------------------------------------------------------

fn smb_client(smb: &SmbProfile) -> Result<SmbClient> {
    let server_url = format!("smb://{}", smb.host);
    let share_info = smb.share.as_deref().unwrap_or("").trim_matches('/');
    let mut creds = SmbCredentials::default().server(&server_url);
    if !share_info.is_empty() {
        creds = creds.share(share_info.to_string());
    }
    if let Some(user) = smb.user.as_deref().filter(|s| !s.trim().is_empty()) {
        creds = creds.username(user);
    }
    if let Some(password) = smb.password.as_deref().filter(|s| !s.trim().is_empty()) {
        creds = creds.password(password);
    }
    if let Some(workgroup) = smb.workgroup.as_deref().filter(|s| !s.trim().is_empty()) {
        creds = creds.workgroup(workgroup);
    }
    let target = if share_info.is_empty() {
        server_url.clone()
    } else {
        format!("{}/{}", server_url, share_info)
    };
    SmbClient::new(creds, SmbOptions::default().one_share_per_server(true)).map_err(|e| {
        anyhow::anyhow!(
            "SMB connection error connecting to {} (user={}, share={}): {}",
            target,
            smb.user.as_deref().unwrap_or("(none)"),
            if share_info.is_empty() {
                "(none)"
            } else {
                share_info
            },
            e
        )
    })
}

fn systemtime_to_local(st: SystemTime) -> Option<DateTime<Local>> {
    let secs = st.duration_since(UNIX_EPOCH).ok()?.as_secs();
    Local.timestamp_opt(secs as i64, 0).single()
}

fn download_smb_path<F>(
    profile: &RemoteProfile,
    remote_path: &str,
    local_target: &Path,
    recursive: bool,
    mut progress: Option<&mut F>,
) -> Result<()>
where
    F: FnMut(&str, u64) -> bool,
{
    let RemoteKind::Smb(smb) = &profile.kind else {
        bail!("Profile is not SMB");
    };
    if recursive {
        // Try directory download; if it fails (not a directory), fall through to file download
        let children = list_smb_dir(profile, remote_path, true);
        if let Ok(children) = children {
            fs::create_dir_all(local_target)?;
            for child in children {
                let child_local = local_target.join(&child.name);
                download_smb_path(
                    profile,
                    &child.path,
                    &child_local,
                    child.is_dir,
                    progress.as_deref_mut(),
                )?;
            }
            return Ok(());
        }
    }
    // File download
    if let Some(parent) = local_target.parent() {
        fs::create_dir_all(parent)?;
    }
    let client = smb_client(smb)?;
    let mut smb_file = client
        .open_with(remote_path, SmbOpenOptions::default().read(true))
        .map_err(|e| anyhow::anyhow!("SMB open error: {e}"))?;
    let mut data = Vec::new();
    smb_file
        .read_to_end(&mut data)
        .map_err(|e| anyhow::anyhow!("SMB read error: {e}"))?;
    drop(smb_file);
    drop(client);
    fs::write(local_target, &data)?;
    let size = data.len() as u64;
    if let Some(cb) = progress.as_mut()
        && !cb(remote_path, size)
    {
        bail!("Aborted");
    }
    Ok(())
}

fn upload_smb_path<F>(
    profile: &RemoteProfile,
    local_path: &Path,
    remote_target: &str,
    recursive: bool,
    mut progress: Option<&mut F>,
) -> Result<()>
where
    F: FnMut(&str, u64) -> bool,
{
    let RemoteKind::Smb(smb) = &profile.kind else {
        bail!("Profile is not SMB");
    };
    if recursive && local_path.is_dir() {
        let client = smb_client(smb)?;
        let _ = client.mkdir(remote_target, SmbMode::from(0o755 as libc::mode_t));
        drop(client);
        for entry in fs::read_dir(local_path)? {
            let entry = entry?;
            let child_local = entry.path();
            let child_remote = join_remote(remote_target, &entry.file_name().to_string_lossy());
            let is_dir = child_local.is_dir();
            upload_smb_path(
                profile,
                &child_local,
                &child_remote,
                is_dir,
                progress.as_deref_mut(),
            )?;
        }
        return Ok(());
    }
    // File upload
    let data = fs::read(local_path)?;
    let size = data.len() as u64;
    let client = smb_client(smb)?;
    let mut smb_file = client
        .open_with(
            remote_target,
            SmbOpenOptions::default()
                .write(true)
                .create(true)
                .truncate(true),
        )
        .map_err(|e| anyhow::anyhow!("SMB open error: {e}"))?;
    smb_file
        .write_all(&data)
        .map_err(|e| anyhow::anyhow!("SMB write error: {e}"))?;
    drop(smb_file);
    drop(client);
    if let Some(cb) = progress.as_mut()
        && !cb(&local_path.to_string_lossy(), size)
    {
        bail!("Aborted");
    }
    Ok(())
}

fn remote_smb_dir_stats_recursive(
    profile: &RemoteProfile,
    remote_path: &str,
) -> Result<RemoteStats> {
    let mut stats = RemoteStats::default();
    for child in list_smb_dir(profile, remote_path, true)? {
        if child.is_dir {
            let sub = remote_smb_dir_stats_recursive(profile, &child.path)?;
            stats.files += sub.files;
            stats.bytes += sub.bytes;
        } else {
            stats.files += 1;
            stats.bytes += child.size;
        }
    }
    Ok(stats)
}

fn scan_smb_dir_recursive<F>(
    profile: &RemoteProfile,
    remote_path: &str,
    progress: &mut F,
    cancel: &Arc<AtomicBool>,
) -> Result<RemoteStats>
where
    F: FnMut(RemoteStats),
{
    let mut stats = RemoteStats::default();
    for child in list_smb_dir(profile, remote_path, true)? {
        if cancel.load(Ordering::Relaxed) {
            bail!("Aborted");
        }
        if child.is_dir {
            let sub = scan_smb_dir_recursive(profile, &child.path, progress, cancel)?;
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
