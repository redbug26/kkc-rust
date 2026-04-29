//! Stub SMB module — compiled when the `smb` feature is **disabled**.
//!
//! Exposes the same types and function signatures as `remote_smb.rs` so that
//! `remote.rs` can call `smb_impl::*` unconditionally, without sprinkling
//! `#[cfg(feature = "smb")]` on every match arm.  All network operations
//! bail immediately with a clear error message; the types are present only
//! for TOML serialisation round-trips (a file written with the `smb` feature
//! can still be parsed without it).

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::{RemoteEntry, RemoteProfile, RemoteStats, SmbProfile};

// ---------------------------------------------------------------------------
// TOML persistence type (mirrors remote_smb.rs)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct SmbProfileToml {
    pub(super) name: String,
    #[serde(default)]
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
// Stub wrappers — all return an error
// ---------------------------------------------------------------------------

pub(super) fn list_smb_shares(_profile: &RemoteProfile) -> Result<Vec<String>> {
    bail!("SMB support not compiled in (rebuild with --features smb)")
}

pub(super) fn smb_rename(_smb: &SmbProfile, _old: &str, _new: &str) -> Result<()> {
    bail!("SMB support not compiled in (rebuild with --features smb)")
}

pub(super) fn smb_mkdir(_smb: &SmbProfile, _path: &str) -> Result<()> {
    bail!("SMB support not compiled in (rebuild with --features smb)")
}

pub(super) fn smb_delete_file(_smb: &SmbProfile, _path: &str) -> Result<()> {
    bail!("SMB support not compiled in (rebuild with --features smb)")
}

pub(super) fn list_smb_dir(
    _profile: &RemoteProfile,
    _cwd: &str,
    _show_hidden: bool,
) -> Result<Vec<RemoteEntry>> {
    bail!("SMB support not compiled in (rebuild with --features smb)")
}

pub(super) fn download_smb_into_dir(
    _profile: &RemoteProfile,
    _remote_path: &str,
    _local_dir: &Path,
    _recursive: bool,
) -> Result<PathBuf> {
    bail!("SMB support not compiled in (rebuild with --features smb)")
}

pub(super) fn upload_smb_into_dir(
    _profile: &RemoteProfile,
    _local_path: &Path,
    _remote_dir: &str,
    _recursive: bool,
) -> Result<String> {
    bail!("SMB support not compiled in (rebuild with --features smb)")
}

pub(super) fn download_smb_with_progress<F>(
    _profile: &RemoteProfile,
    _remote_path: &str,
    _local_dir: &Path,
    _recursive: bool,
    _progress: &mut F,
) -> Result<PathBuf>
where
    F: FnMut(&str, u64) -> bool,
{
    bail!("SMB support not compiled in (rebuild with --features smb)")
}

pub(super) fn upload_smb_with_progress<F>(
    _profile: &RemoteProfile,
    _local_path: &Path,
    _remote_dir: &str,
    _recursive: bool,
    _progress: &mut F,
) -> Result<String>
where
    F: FnMut(&str, u64) -> bool,
{
    bail!("SMB support not compiled in (rebuild with --features smb)")
}

pub(super) fn delete_smb_dir_recursive(_profile: &RemoteProfile, _remote_path: &str) -> Result<()> {
    bail!("SMB support not compiled in (rebuild with --features smb)")
}

pub(super) fn remote_smb_stats(
    _profile: &RemoteProfile,
    _remote_path: &str,
    _is_dir: bool,
) -> Result<RemoteStats> {
    bail!("SMB support not compiled in (rebuild with --features smb)")
}

pub(super) fn scan_smb_stats<F>(
    _profile: &RemoteProfile,
    _remote_path: &str,
    _is_dir: bool,
    _progress: &mut F,
    _cancel: &Arc<AtomicBool>,
) -> Result<RemoteStats>
where
    F: FnMut(RemoteStats),
{
    bail!("SMB support not compiled in (rebuild with --features smb)")
}
