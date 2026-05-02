use super::*;

pub(super) fn panel_config_needs_profiles(cfg: &PanelConfig) -> bool {
    cfg.remote_name.is_some() || cfg.tabs.iter().any(|tab| tab.remote_name.is_some())
}

pub(super) fn cleanup_temp_download(path: &Path) {
    if path.is_dir() {
        let _ = fs::remove_dir_all(path);
    } else {
        let _ = fs::remove_file(path);
        if let Some(parent) = path.parent() {
            let _ = fs::remove_dir(parent);
        }
    }
}

pub(super) fn same_remote_target(a: &RemoteProfile, b: &RemoteProfile) -> bool {
    match (&a.kind, &b.kind) {
        (RemoteKind::Sftp(a), RemoteKind::Sftp(b)) => {
            a.host == b.host
                && a.user == b.user
                && a.port == b.port
                && a.identity_file == b.identity_file
        }
        _ => false,
    }
}

pub(super) fn draw_busy_status(message: &str, has_fkey_bar: bool) -> Result<()> {
    let (_, rows) = size()?;
    if rows == 0 {
        return Ok(());
    }
    let status_row = if has_fkey_bar {
        rows.saturating_sub(2)
    } else {
        rows.saturating_sub(1)
    };
    let mut stdout = io::stdout();
    let line = format!(" {} ", message);
    queue!(
        stdout,
        MoveTo(0, status_row),
        SetForegroundColor(crossterm::style::Color::Rgb {
            r: 244,
            g: 235,
            b: 208
        }),
        SetBackgroundColor(crossterm::style::Color::Rgb {
            r: 125,
            g: 107,
            b: 92
        }),
        Clear(ClearType::CurrentLine),
        Print(line),
        ResetColor,
    )?;
    stdout.flush()?;
    Ok(())
}

pub(super) fn spawn_remote_connect_task(profile: RemoteProfile, show_hidden: bool) -> RemoteConnectTask {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_bg = cancel.clone();
    std::thread::spawn(move || {
        let result = (|| -> Result<(String, Vec<RemoteEntry>)> {
            if cancel_bg.load(Ordering::Relaxed) {
                anyhow::bail!("Aborted");
            }
            let mut progress = |phase: String| {
                let _ = tx.send(RemoteConnectMessage::Progress(phase));
            };
            prepare_connection(&profile, show_hidden, &mut progress, &cancel_bg)
        })();
        match result {
            Ok((cwd, entries)) => {
                let _ = tx.send(RemoteConnectMessage::Connected {
                    profile,
                    cwd,
                    entries,
                });
            }
            Err(err) => {
                if !cancel_bg.load(Ordering::Relaxed) {
                    let _ = tx.send(RemoteConnectMessage::Failed(err.to_string()));
                }
            }
        }
    });
    RemoteConnectTask { rx, cancel }
}
