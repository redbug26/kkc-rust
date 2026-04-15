use crate::file_ops::{self, CopyOptions};
use crate::panel::Entry;
use crate::remote::{
    RemoteProfile, RemoteStats, download_bulk_into_dir, download_with_progress, scan_remote_stats,
    upload_bulk_into_dir, upload_with_progress,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct CopyDialogState {
    pub destination: String,
    pub cursor: usize,
    pub field: usize,
    pub overwrite: bool,
    pub newer_only: bool,
    pub keep_attributes: bool,
    pub file_count: usize,
    pub total_bytes: u64,
    pub stats_pending: bool,
    pub waiting_to_start: bool,
    pub entry_bytes: HashMap<String, u64>,
}

impl CopyDialogState {
    pub const DESTINATION: usize = 0;
    pub const OVERWRITE: usize = 1;
    pub const NEWER_ONLY: usize = 2;
    pub const KEEP_ATTRIBUTES: usize = 3;
    pub const START: usize = 4;
    pub const CANCEL: usize = 5;

    pub fn new(
        destination: String,
        file_count: usize,
        total_bytes: u64,
        stats_pending: bool,
    ) -> Self {
        let cursor = destination.len();
        Self {
            destination,
            cursor,
            field: Self::DESTINATION,
            overwrite: false,
            newer_only: false,
            keep_attributes: false,
            file_count,
            total_bytes,
            stats_pending,
            waiting_to_start: false,
            entry_bytes: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CopyProgressState {
    pub current_name: String,
    pub item_index: usize,
    pub item_count: usize,
    pub file_done: u64,
    pub file_total: u64,
    pub total_done: u64,
    pub total_bytes: u64,
    pub remaining_secs: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct CopyScanUpdate {
    pub stats: RemoteStats,
    pub done: bool,
    pub finished_entry: Option<(String, u64)>,
}

#[derive(Debug)]
pub struct CopyScanTask {
    pub rx: Receiver<CopyScanUpdate>,
    pub cancel: Arc<AtomicBool>,
}

#[derive(Debug)]
pub struct CopyTask {
    pub rx: Receiver<CopyTaskMessage>,
    pub cancel: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
pub enum CopyTaskMessage {
    Progress(CopyProgressState),
    Finished {
        copied_items: usize,
        errors: Vec<String>,
        aborted: bool,
    },
}

#[derive(Debug, Clone)]
pub enum CopySource {
    Local(PathBuf),
    Remote {
        profile: RemoteProfile,
        path: String,
    },
}

#[derive(Debug, Clone)]
pub enum CopyDestination {
    Local(PathBuf),
    Remote { profile: RemoteProfile, cwd: String },
}

#[derive(Debug, Clone)]
pub struct CopyJob {
    pub entry: Entry,
    pub source: CopySource,
    pub total_bytes: u64,
}

pub fn spawn_copy_scan(profile: RemoteProfile, items: Vec<(String, bool)>) -> CopyScanTask {
    let (tx, rx) = mpsc::channel::<CopyScanUpdate>();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_bg = cancel.clone();
    std::thread::spawn(move || {
        let mut total = RemoteStats::default();
        for (path, is_dir) in items {
            if cancel_bg.load(Ordering::Relaxed) {
                break;
            }
            let mut partial = |delta: RemoteStats| {
                total.files += delta.files;
                total.bytes += delta.bytes;
                let _ = tx.send(CopyScanUpdate {
                    stats: total,
                    done: false,
                    finished_entry: None,
                });
            };
            let item_total =
                match scan_remote_stats(&profile, &path, is_dir, &mut partial, &cancel_bg) {
                    Ok(stats) => stats,
                    Err(_) => break,
                };
            let _ = tx.send(CopyScanUpdate {
                stats: total,
                done: false,
                finished_entry: Some((path, item_total.bytes)),
            });
        }
        let _ = tx.send(CopyScanUpdate {
            stats: total,
            done: true,
            finished_entry: None,
        });
    });
    CopyScanTask { rx, cancel }
}

pub fn spawn_copy_task(
    jobs: Vec<CopyJob>,
    destination: CopyDestination,
    options: CopyOptions,
) -> CopyTask {
    let (tx, rx) = mpsc::channel::<CopyTaskMessage>();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_bg = cancel.clone();
    std::thread::spawn(move || {
        let started = Instant::now();
        let total_bytes: u64 = jobs.iter().map(|job| job.total_bytes).sum();
        let item_count = jobs.len();
        let mut total_done = 0u64;
        let mut errors = Vec::new();
        let mut copied_items = 0usize;

        for (idx, job) in jobs.iter().enumerate() {
            if cancel_bg.load(Ordering::Relaxed) {
                let _ = tx.send(CopyTaskMessage::Finished {
                    copied_items,
                    errors,
                    aborted: true,
                });
                return;
            }

            let mut file_done = 0u64;
            let emit = |name: &str, absolute_file_done: u64, total_done_base: u64| {
                let remaining_secs =
                    estimate_remaining(started, total_done_base + absolute_file_done, total_bytes);
                let _ = tx.send(CopyTaskMessage::Progress(CopyProgressState {
                    current_name: name.to_string(),
                    item_index: idx + 1,
                    item_count,
                    file_done: absolute_file_done.min(job.total_bytes),
                    file_total: job.total_bytes,
                    total_done: (total_done_base + absolute_file_done).min(total_bytes),
                    total_bytes,
                    remaining_secs,
                }));
            };

            emit(&job.entry.name, 0, total_done);
            let result = match (&job.source, &destination) {
                (CopySource::Local(src), CopyDestination::Local(dst_dir)) => {
                    let mut cb = |done: u64, _total: u64| -> bool {
                        if cancel_bg.load(Ordering::Relaxed) {
                            return false;
                        }
                        emit(&job.entry.name, done, total_done);
                        true
                    };
                    file_ops::copy_entry_with_options(src, dst_dir, options, Some(&mut cb))
                }
                (CopySource::Local(src), CopyDestination::Remote { profile, cwd }) => {
                    if job.entry.is_dir {
                        upload_bulk_into_dir(profile, src, cwd).map(|_| ())
                    } else {
                        let mut cb = |name: &str, bytes: u64| -> bool {
                            if cancel_bg.load(Ordering::Relaxed) {
                                return false;
                            }
                            file_done += bytes;
                            emit(name, file_done, total_done);
                            true
                        };
                        upload_with_progress(profile, src, cwd, false, &mut cb).map(|_| ())
                    }
                }
                (CopySource::Remote { profile, path }, CopyDestination::Local(dst_dir)) => {
                    if job.entry.is_dir {
                        download_bulk_into_dir(profile, path, dst_dir).map(|_| ())
                    } else {
                        let mut cb = |name: &str, bytes: u64| -> bool {
                            if cancel_bg.load(Ordering::Relaxed) {
                                return false;
                            }
                            file_done += bytes;
                            emit(name, file_done, total_done);
                            true
                        };
                        download_with_progress(profile, path, dst_dir, false, &mut cb).map(|_| ())
                    }
                }
                (
                    CopySource::Remote {
                        profile: src_profile,
                        path,
                    },
                    CopyDestination::Remote {
                        profile: dst_profile,
                        cwd,
                    },
                ) => {
                    let temp_dir = std::env::temp_dir().join("kkc-copy-worker").join(format!(
                        "{}-{}",
                        std::process::id(),
                        idx
                    ));
                    let mut cb = |name: &str, bytes: u64| -> bool {
                        if cancel_bg.load(Ordering::Relaxed) {
                            return false;
                        }
                        file_done += bytes;
                        emit(name, file_done.min(job.total_bytes), total_done);
                        true
                    };
                    let res = if job.entry.is_dir {
                        download_bulk_into_dir(src_profile, path, &temp_dir).and_then(|tmp_path| {
                            upload_bulk_into_dir(dst_profile, &tmp_path, cwd)?;
                            cleanup_temp_download(&tmp_path);
                            Ok(())
                        })
                    } else {
                        download_with_progress(src_profile, path, &temp_dir, false, &mut cb)
                            .and_then(|tmp_path| {
                                upload_with_progress(dst_profile, &tmp_path, cwd, false, &mut cb)?;
                                cleanup_temp_download(&tmp_path);
                                Ok(())
                            })
                    };
                    let _ = std::fs::remove_dir_all(&temp_dir);
                    res
                }
            };

            match result {
                Ok(()) => {
                    total_done += job.total_bytes;
                    copied_items += 1;
                    emit(
                        &job.entry.name,
                        job.total_bytes,
                        total_done - job.total_bytes,
                    );
                }
                Err(err) if is_abort_error(&err) || cancel_bg.load(Ordering::Relaxed) => {
                    let _ = tx.send(CopyTaskMessage::Finished {
                        copied_items,
                        errors,
                        aborted: true,
                    });
                    return;
                }
                Err(err) => {
                    errors.push(format!("{}: {}", job.entry.name, err));
                    total_done += job.total_bytes;
                }
            }
        }

        let _ = tx.send(CopyTaskMessage::Finished {
            copied_items,
            errors,
            aborted: false,
        });
    });

    CopyTask { rx, cancel }
}

pub fn count_local_files(path: &std::path::Path) -> usize {
    if path.is_dir() {
        walkdir::WalkDir::new(path)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .count()
    } else {
        1
    }
}

pub fn is_abort_error(err: &anyhow::Error) -> bool {
    err.to_string().contains("Aborted")
}

fn estimate_remaining(started: Instant, done: u64, total: u64) -> Option<u64> {
    if done == 0 || total <= done {
        return None;
    }
    let elapsed = started.elapsed().as_secs_f64();
    Some(((elapsed / done as f64) * (total - done) as f64).max(0.0) as u64)
}

fn cleanup_temp_download(path: &std::path::Path) {
    if path.is_dir() {
        let _ = std::fs::remove_dir_all(path);
    } else {
        let _ = std::fs::remove_file(path);
        if let Some(parent) = path.parent() {
            let _ = std::fs::remove_dir_all(parent);
        }
    }
}
