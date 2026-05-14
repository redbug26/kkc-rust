use super::*;

pub(super) fn ranked_filtered_indices<T, Include, Searchable, Starts>(
    items: &[T],
    query: &str,
    include: Include,
    searchable: Searchable,
    starts_with_first: Starts,
) -> Vec<usize>
where
    Include: Fn(&T) -> bool,
    Searchable: Fn(&T) -> String,
    Starts: Fn(&T, &str, &str) -> bool,
{
    if query.trim().is_empty() {
        return items
            .iter()
            .enumerate()
            .filter_map(|(idx, item)| include(item).then_some(idx))
            .collect();
    }

    let tokens: Vec<String> = query
        .split_whitespace()
        .map(|token| token.to_lowercase())
        .filter(|token| !token.is_empty())
        .collect();
    if tokens.is_empty() {
        return items
            .iter()
            .enumerate()
            .filter_map(|(idx, item)| include(item).then_some(idx))
            .collect();
    }

    let first = &tokens[0];
    let rest = &tokens[1..];
    let mut starts = Vec::new();
    let mut contains = Vec::new();

    for (idx, item) in items.iter().enumerate() {
        if !include(item) {
            continue;
        }
        let lowered = searchable(item).to_lowercase();
        if !rest.iter().all(|token| lowered.contains(token.as_str())) {
            continue;
        }
        if starts_with_first(item, first, &lowered) {
            starts.push(idx);
        } else if lowered.contains(first.as_str()) {
            contains.push(idx);
        }
    }

    starts.extend(contains);
    starts
}

pub(super) fn clamp_index(index: &mut usize, len: usize) {
    if len == 0 {
        *index = 0;
    } else {
        *index = (*index).min(len.saturating_sub(1));
    }
}

pub(super) fn move_index_prev_wrapping(index: &mut usize, len: usize) {
    if len == 0 {
        *index = 0;
    } else if *index == 0 {
        *index = len - 1;
    } else {
        *index -= 1;
    }
}

pub(super) fn move_index_next_wrapping(index: &mut usize, len: usize) {
    if len == 0 {
        *index = 0;
    } else {
        *index = (*index + 1) % len;
    }
}

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

pub(super) fn spawn_remote_connect_task(
    profile: RemoteProfile,
    show_hidden: bool,
) -> RemoteConnectTask {
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
