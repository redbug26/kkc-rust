use anyhow::{Context, Result, anyhow};
use kkc_plugin_api::{AudioPlaybackSnapshot as PluginSnapshot, AudioPluginModRef};
use rodio::Source;
use rustfft::{FftPlanner, num_complex::Complex};
use std::collections::HashMap;
use std::io::Cursor;
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use xmrs::duration::ModuleDuration;
use xmrs::fixed::units::Amplification;
use xmrs::prelude::*;
use xmrsplayer::prelude::XmrsPlayer;

const SAMPLE_RATE: u32 = 48_000;
const BUFFER_SIZE: usize = 2048;
const SPECTRUM_BANDS: usize = 96;

static AUDIO_PLUGIN_HINTS: OnceLock<Mutex<HashMap<PathBuf, String>>> = OnceLock::new();

fn audio_plugin_hints() -> &'static Mutex<HashMap<PathBuf, String>> {
    AUDIO_PLUGIN_HINTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn remember_audio_plugin_for_path(path: &Path, plugin_id: &str) {
    if let Ok(mut hints) = audio_plugin_hints().lock() {
        hints.insert(path.to_path_buf(), plugin_id.to_string());
    }
}

fn hinted_audio_plugin_for_path(path: &Path) -> Option<String> {
    audio_plugin_hints()
        .lock()
        .ok()
        .and_then(|hints| hints.get(path).cloned())
}

#[derive(Debug, Clone)]
pub struct TrackerModuleInfo {
    pub name: String,
    pub format: String,
    pub songs: usize,
    pub channels: usize,
    pub sample_rate: Option<u32>,
    pub duration: Option<Duration>,
    pub orders: Vec<usize>,
    pub patterns: Vec<Vec<String>>,
    pub text_tracks: Vec<String>,
}

struct PlaybackState {
    path: PathBuf,
    _stream: rodio::MixerDeviceSink,
    player: rodio::Player,
    visualizer: Arc<Mutex<TrackerVisualizer>>,
    duration: Option<Duration>,
    plugin_backend: Option<PluginBackendState>,
}

struct PluginBackendState {
    module: AudioPluginModRef,
    path: String,
}

#[derive(Debug, Clone)]
pub struct TrackerPlaybackSnapshot {
    pub rms: f32,
    pub spectrum: Vec<f32>,
    pub table_index: usize,
    pub pattern: usize,
    pub row: usize,
    pub playing: bool,
    pub position: Duration,
    pub duration: Option<Duration>,
    pub tracker_monitor_lines: Vec<String>,
    pub track_text_lines: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct TrackerVisualizer {
    samples: Vec<f32>,
    write_pos: usize,
    rms: f32,
    spectrum: Vec<f32>,
    table_index: usize,
    pattern: usize,
    row: usize,
    playing: bool,
    position: Option<Duration>,
    duration: Option<Duration>,
    tracker_monitor_lines: Vec<String>,
    track_text_lines: Vec<String>,
}

impl TrackerVisualizer {
    pub(crate) fn new() -> Self {
        Self {
            samples: vec![0.0; 1024],
            write_pos: 0,
            rms: 0.0,
            spectrum: vec![0.0; SPECTRUM_BANDS],
            table_index: 0,
            pattern: 0,
            row: 0,
            playing: true,
            position: None,
            duration: None,
            tracker_monitor_lines: Vec::new(),
            track_text_lines: Vec::new(),
        }
    }

    pub(crate) fn update(
        &mut self,
        samples: &[f32],
        table_index: usize,
        pattern: usize,
        row: usize,
    ) {
        for &sample in samples {
            self.samples[self.write_pos] = sample;
            self.write_pos = (self.write_pos + 1) % self.samples.len();
        }
        self.table_index = table_index;
        self.pattern = pattern;
        self.row = row;
        self.playing = true;
        self.position = None;
        self.duration = None;
        self.tracker_monitor_lines.clear();
        self.track_text_lines.clear();
        self.rms = if samples.is_empty() {
            0.0
        } else {
            (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
        };
        self.spectrum = compute_fft_bands(&self.ordered_samples(), SPECTRUM_BANDS);
    }

    fn update_plugin(&mut self, samples: &[f32], snapshot: &PluginSnapshot) {
        for &sample in samples {
            self.samples[self.write_pos] = sample;
            self.write_pos = (self.write_pos + 1) % self.samples.len();
        }
        self.table_index = snapshot.table_index as usize;
        self.pattern = snapshot.pattern as usize;
        self.row = snapshot.row as usize;
        self.playing = snapshot.playing;
        self.position = Some(Duration::from_secs_f64(snapshot.position_secs.max(0.0)));
        self.duration = if snapshot.duration_secs > 0.0 {
            Some(Duration::from_secs_f64(snapshot.duration_secs))
        } else {
            None
        };
        self.tracker_monitor_lines = snapshot
            .tracker_monitor_lines
            .iter()
            .map(|line| line.to_string())
            .collect();
        self.track_text_lines = snapshot
            .track_text_lines
            .iter()
            .map(|line| line.to_string())
            .collect();
        self.rms = snapshot.rms;
        self.spectrum = snapshot.spectrum.iter().copied().collect();
        if self.spectrum.is_empty() {
            self.spectrum = compute_fft_bands(&self.ordered_samples(), SPECTRUM_BANDS);
        }
    }

    fn ordered_samples(&self) -> Vec<f32> {
        self.samples[self.write_pos..]
            .iter()
            .chain(self.samples[..self.write_pos].iter())
            .copied()
            .collect()
    }

    fn snapshot(&self) -> TrackerPlaybackSnapshot {
        TrackerPlaybackSnapshot {
            rms: self.rms,
            spectrum: self.spectrum.clone(),
            table_index: self.table_index,
            pattern: self.pattern,
            row: self.row,
            playing: self.playing,
            position: self.position.unwrap_or(Duration::ZERO),
            duration: self.duration,
            tracker_monitor_lines: self.tracker_monitor_lines.clone(),
            track_text_lines: self.track_text_lines.clone(),
        }
    }

    pub(crate) fn stop(&mut self) {
        self.playing = false;
    }
}

fn playback_state() -> &'static Mutex<Option<PlaybackState>> {
    static STATE: OnceLock<Mutex<Option<PlaybackState>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(None))
}

pub fn is_tracker_module_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "mod" | "xm" | "s3m" | "it"
            )
        })
        .unwrap_or(false)
}

fn decoded_audio_format(path: &Path) -> Option<&'static str> {
    path.extension()
        .and_then(|ext| ext.to_str())
        .and_then(|ext| match ext.to_ascii_lowercase().as_str() {
            "wav" => Some("WAV"),
            _ => None,
        })
}

fn select_audio_plugin(
    path: &Path,
    preferred_plugin_ids: &[String],
) -> Option<(crate::audio_plugins::AudioRustPluginInfo, AudioPluginModRef)> {
    let path_text = path.to_string_lossy();
    let path_mime_types = crate::idf::probe_path(path).map(|info| {
        info.mime_types
            .into_iter()
            .map(|mime_type| mime_type.to_ascii_lowercase())
            .collect::<Vec<_>>()
    });
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .unwrap_or_default();
    let plugins_dir = crate::plugins::plugins_dir().ok()?;
    let discovered = crate::audio_plugins::discover_audio_rust_plugins(&plugins_dir).ok()?;

    crate::viewer::debug_log(&format!(
        "audio: discovered {} plugins for '{}'",
        discovered.len(),
        path.display()
    ));

    let mut tried_preferred = Vec::new();
    for preferred_id in preferred_plugin_ids {
        if tried_preferred.iter().any(|id: &String| id == preferred_id) {
            continue;
        }
        tried_preferred.push(preferred_id.clone());
        let Some(plugin) = discovered.iter().find(|plugin| plugin.id == *preferred_id) else {
            continue;
        };
        let Some(module) = try_load_audio_plugin(path, &path_text, plugin) else {
            continue;
        };
        return Some((plugin.clone(), module));
    }

    for plugin in discovered {
        let mime_match = path_mime_types.as_deref().is_some_and(|mime_types| {
            !plugin.mime_types.is_empty()
                && mime_types.iter().any(|mime_type| {
                    plugin
                        .mime_types
                        .iter()
                        .any(|candidate| candidate == mime_type)
                })
        });
        let legacy_ext_match = !plugin.extensions.is_empty()
            && !extension.is_empty()
            && plugin.extensions.iter().any(|ext| ext == &extension);
        if !mime_match && !legacy_ext_match {
            continue;
        }
        let module = match crate::audio_plugins::load_audio_plugin(&plugin.id) {
            Ok(module) => module,
            Err(err) => {
                crate::viewer::debug_log(&format!(
                    "audio: failed to load plugin '{}' for '{}': {err}",
                    plugin.id,
                    path.display()
                ));
                continue;
            }
        };
        let supported = match module.probe()(path_text.as_ref().into()).into_result() {
            Ok(supported) => supported,
            Err(err) => {
                crate::viewer::debug_log(&format!(
                    "audio: plugin '{}' probe failed for '{}': {err}",
                    plugin.id,
                    path.display()
                ));
                false
            }
        };
        crate::viewer::debug_log(&format!(
            "audio: plugin '{}' probe '{}' => {}",
            plugin.id,
            path.display(),
            supported
        ));
        if supported {
            return Some((plugin, module));
        }
    }
    None
}

fn try_load_audio_plugin(
    path: &Path,
    path_text: &std::borrow::Cow<'_, str>,
    plugin: &crate::audio_plugins::AudioRustPluginInfo,
) -> Option<AudioPluginModRef> {
    let module = match crate::audio_plugins::load_audio_plugin(&plugin.id) {
        Ok(module) => module,
        Err(err) => {
            crate::viewer::debug_log(&format!(
                "audio: failed to load preferred plugin '{}' for '{}': {err}",
                plugin.id,
                path.display()
            ));
            return None;
        }
    };

    let supported = match module.probe()(path_text.as_ref().into()).into_result() {
        Ok(supported) => supported,
        Err(err) => {
            crate::viewer::debug_log(&format!(
                "audio: preferred plugin '{}' probe failed for '{}': {err}",
                plugin.id,
                path.display()
            ));
            false
        }
    };
    crate::viewer::debug_log(&format!(
        "audio: preferred plugin '{}' probe '{}' => {}",
        plugin.id,
        path.display(),
        supported
    ));
    supported.then_some(module)
}

pub fn preferred_audio_plugin_ids_for_path(path: &Path) -> Vec<String> {
    let Some(config) = crate::config::Config::load().ok() else {
        return Vec::new();
    };
    let Some(info) = crate::idf::probe_path(path) else {
        return Vec::new();
    };

    let mut plugin_ids = Vec::new();
    for mime_type in info.mime_types {
        if let Some(plugin_id) = config.audio_player_for_mime(&mime_type)
            && !plugin_ids.iter().any(|existing| existing == plugin_id)
        {
            plugin_ids.push(plugin_id.to_string());
        }
    }
    plugin_ids
}

fn map_plugin_info(path: &Path, info: &kkc_plugin_api::AudioTrackInfo) -> TrackerModuleInfo {
    TrackerModuleInfo {
        name: info.name.to_string(),
        format: info.format.to_string(),
        songs: info.songs as usize,
        channels: info.channels as usize,
        sample_rate: Some(info.sample_rate),
        duration: if info.duration_secs > 0.0 {
            Some(Duration::from_secs_f64(info.duration_secs))
        } else {
            None
        },
        orders: Vec::new(),
        patterns: Vec::new(),
        text_tracks: if info.tracker_text_lines.is_empty() {
            vec![format!("Plugin audio: {}", path.display())]
        } else {
            info.tracker_text_lines
                .iter()
                .map(|line| line.to_string())
                .collect()
        },
    }
}

pub fn is_audio_path(path: &Path) -> bool {
    crate::viewer::debug_log(&format!(
        "audio: checking if '{}' is audio path",
        path.display()
    ));

    is_tracker_module_path(path)
        || decoded_audio_format(path).is_some()
        || select_audio_plugin(path, &preferred_audio_plugin_ids_for_path(path)).is_some()
}

pub fn module_info(bytes: &[u8]) -> Result<TrackerModuleInfo> {
    let module = Module::load(bytes).map_err(|err| anyhow!("Loading tracker module: {err:?}"))?;
    Ok(info_from_module(&module))
}

pub fn try_audio_info(path: &Path, bytes: &[u8]) -> Option<TrackerModuleInfo> {
    crate::viewer::debug_log(&format!(
        "audio: trying to get info for '{}' as audio path",
        path.display()
    ));

    if let Some((plugin, module)) =
        select_audio_plugin(path, &preferred_audio_plugin_ids_for_path(path))
    {
        remember_audio_plugin_for_path(path, &plugin.id);
        let path_text = path.to_string_lossy();
        let info = module.open()(path_text.as_ref().into())
            .into_result()
            .ok()?;
        let _ = module.close()(path_text.as_ref().into());
        Some(map_plugin_info(path, &info))
    } else if decoded_audio_format(path).is_some() {
        decoded_audio_info(path, bytes).ok()
    } else if is_tracker_module_path(path) {
        module_info(bytes).ok()
    } else {
        None
    }
}

pub fn playback_snapshot_for_path(path: &Path) -> Option<TrackerPlaybackSnapshot> {
    let state = playback_state().lock().ok()?;
    let state = state.as_ref()?;
    if state.path != path {
        return None;
    }
    state.visualizer.lock().ok().map(|v| {
        let mut snapshot = v.snapshot();
        if snapshot.position.is_zero() {
            snapshot.position = state.player.get_pos();
        }
        if snapshot.duration.is_none() {
            snapshot.duration = state.duration;
        }
        snapshot.playing = !state.player.empty();
        snapshot
    })
}

pub fn playback_finished_for_path(path: &Path) -> bool {
    let Some(state_guard) = playback_state().lock().ok() else {
        return false;
    };
    let Some(state) = state_guard.as_ref() else {
        return false;
    };
    if state.path != path {
        return false;
    }
    if let Some(plugin) = &state.plugin_backend {
        return plugin.module.is_finished()(plugin.path.as_str().into())
            .into_result()
            .unwrap_or(false);
    }
    state.player.empty()
}

pub fn play_audio_file(path: &Path) -> Result<TrackerModuleInfo> {
    let bytes = std::fs::read(path).with_context(|| format!("Reading {}", path.display()))?;
    play_audio_bytes(path.to_path_buf(), &bytes)
}

pub fn play_audio_bytes(path: PathBuf, bytes: &[u8]) -> Result<TrackerModuleInfo> {
    let preferred_plugin_ids = preferred_audio_plugin_ids_for_path(&path);
    play_audio_bytes_with_preferences(path, bytes, &preferred_plugin_ids)
}

pub fn play_audio_bytes_with_preferred_plugin(
    path: PathBuf,
    bytes: &[u8],
    preferred_plugin_id: &str,
) -> Result<TrackerModuleInfo> {
    let mut preferred_plugin_ids = vec![preferred_plugin_id.to_string()];
    preferred_plugin_ids.extend(preferred_audio_plugin_ids_for_path(&path));
    preferred_plugin_ids.dedup();
    play_audio_bytes_with_preferences(path, bytes, &preferred_plugin_ids)
}

fn play_audio_bytes_with_preferences(
    path: PathBuf,
    bytes: &[u8],
    preferred_plugin_ids: &[String],
) -> Result<TrackerModuleInfo> {
    crate::viewer::debug_log(&format!(
        "audio: attempting to play '{}' with preferred plugins: {:?}",
        path.display(),
        preferred_plugin_ids
    ));

    // If info probing already selected a plugin for this path, try it directly
    // to avoid re-probing the exact same file in the same open flow.
    if preferred_plugin_ids.is_empty()
        && let Some(hinted_plugin_id) = hinted_audio_plugin_for_path(&path)
        && let Ok(plugins_dir) = crate::plugins::plugins_dir()
        && let Ok(discovered) = crate::audio_plugins::discover_audio_rust_plugins(&plugins_dir)
        && let Some(plugin_info) = discovered
            .iter()
            .find(|plugin| plugin.id == hinted_plugin_id)
            .cloned()
        && let Ok(module) = crate::audio_plugins::load_audio_plugin(&hinted_plugin_id)
    {
        return play_audio_plugin_path(path, plugin_info, module);
    }

    if let Some((plugin_info, module)) = select_audio_plugin(&path, preferred_plugin_ids) {
        remember_audio_plugin_for_path(&path, &plugin_info.id);
        play_audio_plugin_path(path, plugin_info, module)
    } else if decoded_audio_format(&path).is_some() {
        play_decoded_audio_bytes(path, bytes)
    } else {
        play_tracker_bytes(path, bytes)
    }
}

fn play_tracker_bytes(path: PathBuf, bytes: &[u8]) -> Result<TrackerModuleInfo> {
    let module = Module::load(bytes).map_err(|err| anyhow!("Loading tracker module: {err:?}"))?;
    let mut info = info_from_module(&module);
    info.duration = Some(module.duration(0));

    let module_ref: &'static Module = Box::leak(Box::new(module));
    let mut xmrs_player = XmrsPlayer::new(module_ref, SAMPLE_RATE, 0);
    xmrs_player.set_amplification(Amplification::from_raw_q4_12((0.30 * 4096.0) as i16));
    xmrs_player.set_max_loop_count(1);

    let player = Arc::new(Mutex::new(xmrs_player));
    let visualizer = Arc::new(Mutex::new(TrackerVisualizer::new()));
    let source = TrackerSource::new(Arc::clone(&player), Arc::clone(&visualizer), info.duration);
    let mut stream = rodio::DeviceSinkBuilder::open_default_sink()
        .map_err(|err| anyhow!("Opening default audio output: {err}"))?;
    stream.log_on_drop(false);
    let sink = rodio::Player::connect_new(&stream.mixer());
    sink.append(source);
    sink.play();

    let mut state = playback_state()
        .lock()
        .map_err(|_| anyhow!("Tracker audio state lock poisoned"))?;
    if let Some(old) = state.take() {
        if let Some(plugin) = old.plugin_backend.as_ref() {
            let _ = plugin.module.close()(plugin.path.as_str().into());
        }
        old.player.stop();
    }
    *state = Some(PlaybackState {
        path,
        _stream: stream,
        player: sink,
        visualizer,
        duration: info.duration,
        plugin_backend: None,
    });

    Ok(info)
}

fn play_decoded_audio_bytes(path: PathBuf, bytes: &[u8]) -> Result<TrackerModuleInfo> {
    let info = decoded_audio_info(&path, bytes)?;
    let decoder = rodio::Decoder::try_from(Cursor::new(bytes.to_vec()))
        .map_err(|err| anyhow!("Decoding audio: {err}"))?;
    let visualizer = Arc::new(Mutex::new(TrackerVisualizer::new()));
    let source = DecodedAudioVisualizerSource::new(decoder, Arc::clone(&visualizer));
    let mut stream = rodio::DeviceSinkBuilder::open_default_sink()
        .map_err(|err| anyhow!("Opening default audio output: {err}"))?;
    stream.log_on_drop(false);
    let sink = rodio::Player::connect_new(&stream.mixer());
    sink.append(source);
    sink.play();

    let mut state = playback_state()
        .lock()
        .map_err(|_| anyhow!("Tracker audio state lock poisoned"))?;
    if let Some(old) = state.take() {
        if let Some(plugin) = old.plugin_backend.as_ref() {
            let _ = plugin.module.close()(plugin.path.as_str().into());
        }
        old.player.stop();
    }
    *state = Some(PlaybackState {
        path,
        _stream: stream,
        player: sink,
        visualizer,
        duration: info.duration,
        plugin_backend: None,
    });

    Ok(info)
}

fn play_audio_plugin_path(
    path: PathBuf,
    plugin_info: crate::audio_plugins::AudioRustPluginInfo,
    module: AudioPluginModRef,
) -> Result<TrackerModuleInfo> {
    let path_text = path.to_string_lossy().to_string();
    crate::viewer::debug_log(&format!(
        "audio: opening plugin '{}' for '{}'",
        plugin_info.id,
        path.display()
    ));
    let info = module.open()(path_text.as_str().into())
        .into_result()
        .map_err(|err| {
            crate::viewer::debug_log(&format!(
                "audio: plugin '{}' open failed for '{}': {err}",
                plugin_info.id,
                path.display()
            ));
            anyhow!("Opening audio plugin track: {err}")
        })?;
    let info = map_plugin_info(&path, &info);

    let visualizer = Arc::new(Mutex::new(TrackerVisualizer::new()));
    let source = PluginAudioSource::new(
        module,
        path_text.clone(),
        Arc::clone(&visualizer),
        info.duration,
        info.channels as u16,
        info.sample_rate.unwrap_or(SAMPLE_RATE),
    )?;

    let mut stream = rodio::DeviceSinkBuilder::open_default_sink()
        .map_err(|err| anyhow!("Opening default audio output: {err}"))?;
    stream.log_on_drop(false);
    let sink = rodio::Player::connect_new(&stream.mixer());
    sink.append(source);
    sink.play();

    let mut state = playback_state()
        .lock()
        .map_err(|_| anyhow!("Tracker audio state lock poisoned"))?;
    if let Some(old) = state.take() {
        if let Some(plugin) = old.plugin_backend.as_ref() {
            let _ = plugin.module.close()(plugin.path.as_str().into());
        }
        old.player.stop();
    }
    *state = Some(PlaybackState {
        path,
        _stream: stream,
        player: sink,
        visualizer,
        duration: info.duration,
        plugin_backend: Some(PluginBackendState {
            module,
            path: path_text,
        }),
    });

    Ok(info)
}

pub fn stop_module() {
    if let Ok(mut state) = playback_state().lock()
        && let Some(old) = state.take()
    {
        if let Some(plugin) = old.plugin_backend.as_ref() {
            let _ = plugin.module.close()(plugin.path.as_str().into());
        }
        old.player.stop();
    }
}

pub fn stop_module_if_path(path: &Path) {
    if let Ok(mut state) = playback_state().lock() {
        let should_stop = state
            .as_ref()
            .map(|current| current.path == path)
            .unwrap_or(false);
        if should_stop && let Some(old) = state.take() {
            if let Some(plugin) = old.plugin_backend.as_ref() {
                let _ = plugin.module.close()(plugin.path.as_str().into());
            }
            old.player.stop();
        }
    }
}

pub fn is_module_playing() -> bool {
    playback_state()
        .lock()
        .map(|state| state.is_some())
        .unwrap_or(false)
}

fn info_from_module(module: &Module) -> TrackerModuleInfo {
    TrackerModuleInfo {
        name: module.name.trim().to_string(),
        format: format!("{:?}", module.profile.format),
        songs: module.pattern_order.len(),
        channels: module.get_num_channels(),
        sample_rate: None,
        duration: None,
        orders: module.pattern_order.first().cloned().unwrap_or_default(),
        patterns: module
            .pattern
            .iter()
            .map(|pattern| {
                pattern
                    .iter()
                    .enumerate()
                    .map(|(row_idx, row)| format_pattern_row(row_idx, row))
                    .collect::<Vec<_>>()
            })
            .collect(),
        text_tracks: module_text_tracks(module),
    }
}

fn decoded_audio_info(path: &Path, bytes: &[u8]) -> Result<TrackerModuleInfo> {
    let decoder = rodio::Decoder::try_from(Cursor::new(bytes.to_vec()))
        .map_err(|err| anyhow!("Reading audio metadata: {err}"))?;
    Ok(TrackerModuleInfo {
        name: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Untitled")
            .to_string(),
        format: decoded_audio_format(path).unwrap_or("Audio").into(),
        songs: 1,
        channels: decoder.channels().get() as usize,
        sample_rate: Some(decoder.sample_rate().get()),
        duration: decoder.total_duration(),
        orders: Vec::new(),
        patterns: Vec::new(),
        text_tracks: Vec::new(),
    })
}

fn module_text_tracks(module: &Module) -> Vec<String> {
    let mut lines = Vec::new();

    let comment = module.comment.trim();
    if !comment.is_empty() {
        lines.push("Comment".into());
        lines.extend(comment.lines().map(|line| format!("  {line}")));
    }

    let channel_names = module
        .channel_names
        .iter()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    if !channel_names.is_empty() {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push("Channels".into());
        for (idx, name) in channel_names.iter().enumerate() {
            lines.push(format!("  {:02}. {}", idx + 1, name));
        }
    }

    let mut instruments = Vec::new();
    let mut samples = Vec::new();
    for (idx, instrument) in module.instrument.iter().enumerate() {
        let name = instrument.name.trim();
        if !name.is_empty() {
            instruments.push(format!("  {:02}. {}", idx + 1, name));
        }
        if let InstrumentType::Default(default) = &instrument.instr_type {
            for (sample_idx, sample) in default.sample.iter().enumerate() {
                let Some(sample) = sample else {
                    continue;
                };
                let sample_name = sample.name.trim();
                if !sample_name.is_empty() {
                    samples.push(format!(
                        "  {:02}.{:02} {}",
                        idx + 1,
                        sample_idx + 1,
                        sample_name
                    ));
                }
            }
        }
    }

    if !instruments.is_empty() {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push("Instruments".into());
        lines.extend(instruments);
    }

    if !samples.is_empty() {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push("Samples".into());
        lines.extend(samples);
    }

    lines
}

fn format_pattern_row(row_idx: usize, row: &[TrackUnit]) -> String {
    let mut line = format!("{row_idx:02X} ");
    for unit in row {
        let note = format!("{:?}", unit.note);
        let instr = unit
            .instrument
            .map(|instr| format!("{instr:02X}"))
            .unwrap_or_else(|| "..".into());
        let fx = unit
            .effects
            .first()
            .map(|fx| format!("{fx:?}"))
            .unwrap_or_default();
        let fx = fx.chars().take(3).collect::<String>();
        line.push_str(&format!("[{note:<4} {instr} {fx:<3}] "));
    }
    line
}

struct DecodedAudioVisualizerSource {
    inner: rodio::Decoder<Cursor<Vec<u8>>>,
    visualizer: Arc<Mutex<TrackerVisualizer>>,
    channels: NonZero<u16>,
    sample_rate: NonZero<u32>,
    frame: Vec<f32>,
    mono: Vec<f32>,
}

impl DecodedAudioVisualizerSource {
    fn new(
        inner: rodio::Decoder<Cursor<Vec<u8>>>,
        visualizer: Arc<Mutex<TrackerVisualizer>>,
    ) -> Self {
        let channels = inner.channels();
        let sample_rate = inner.sample_rate();
        Self {
            inner,
            visualizer,
            channels,
            sample_rate,
            frame: Vec::with_capacity(channels.get() as usize),
            mono: Vec::with_capacity(512),
        }
    }

    fn push_sample(&mut self, sample: f32) {
        self.frame.push(sample);
        if self.frame.len() >= self.channels.get() as usize {
            self.mono
                .push(self.frame.iter().sum::<f32>() / self.frame.len() as f32);
            self.frame.clear();
        }
        if self.mono.len() >= 512
            && let Ok(mut visualizer) = self.visualizer.lock()
        {
            visualizer.update(&self.mono, 0, 0, 0);
            self.mono.clear();
        }
    }
}

impl Iterator for DecodedAudioVisualizerSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.inner.next()?;
        self.push_sample(sample);
        Some(sample)
    }
}

impl Source for DecodedAudioVisualizerSource {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> NonZero<u16> {
        self.channels
    }

    fn sample_rate(&self) -> NonZero<u32> {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        self.inner.total_duration()
    }
}

struct PluginAudioSource {
    module: AudioPluginModRef,
    path: String,
    visualizer: Arc<Mutex<TrackerVisualizer>>,
    channels: NonZero<u16>,
    sample_rate: NonZero<u32>,
    duration: Option<Duration>,
    finished: bool,
    buffer: Vec<f32>,
    buffer_index: usize,
}

impl PluginAudioSource {
    fn new(
        module: AudioPluginModRef,
        path: String,
        visualizer: Arc<Mutex<TrackerVisualizer>>,
        duration: Option<Duration>,
        channels: u16,
        sample_rate: u32,
    ) -> Result<Self> {
        let channels = NonZero::new(channels.max(1)).ok_or_else(|| anyhow!("Invalid channels"))?;
        let sample_rate =
            NonZero::new(sample_rate.max(1)).ok_or_else(|| anyhow!("Invalid sample rate"))?;
        Ok(Self {
            module,
            path,
            visualizer,
            channels,
            sample_rate,
            duration,
            finished: false,
            buffer: Vec::new(),
            buffer_index: 0,
        })
    }

    fn refill(&mut self) {
        if self.finished {
            self.buffer.clear();
            self.buffer_index = 0;
            if let Ok(mut visualizer) = self.visualizer.lock() {
                visualizer.stop();
            }
            return;
        }

        let chunk = match self.module.read_samples()(self.path.as_str().into(), 1024).into_result()
        {
            Ok(chunk) => chunk,
            Err(_) => {
                self.finished = true;
                self.buffer.clear();
                self.buffer_index = 0;
                if let Ok(mut visualizer) = self.visualizer.lock() {
                    visualizer.stop();
                }
                return;
            }
        };

        self.finished = chunk.finished;
        self.buffer = chunk.samples.iter().copied().collect();
        self.buffer_index = 0;

        let channels = chunk.channels.max(1) as usize;
        if chunk.channels > 0 {
            self.channels = NonZero::new(chunk.channels as u16).unwrap_or(self.channels);
        }
        if chunk.sample_rate > 0 {
            self.sample_rate = NonZero::new(chunk.sample_rate).unwrap_or(self.sample_rate);
        }

        let mut mono = Vec::with_capacity(self.buffer.len().saturating_div(channels).max(1));
        for frame in self.buffer.chunks(channels) {
            if frame.is_empty() {
                continue;
            }
            mono.push(frame.iter().sum::<f32>() / frame.len() as f32);
        }

        if let Ok(snapshot) = self.module.snapshot()(self.path.as_str().into()).into_result() {
            if let Ok(mut visualizer) = self.visualizer.lock() {
                visualizer.update_plugin(&mono, &snapshot);
            }
        } else if let Ok(mut visualizer) = self.visualizer.lock() {
            visualizer.update(&mono, 0, 0, 0);
        }
    }
}

impl Iterator for PluginAudioSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.buffer_index >= self.buffer.len() {
            self.refill();
            if self.buffer.is_empty() {
                return None;
            }
        }
        let sample = self.buffer[self.buffer_index];
        self.buffer_index += 1;
        Some(sample)
    }
}

impl Source for PluginAudioSource {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.buffer.len().saturating_sub(self.buffer_index))
    }

    fn channels(&self) -> NonZero<u16> {
        self.channels
    }

    fn sample_rate(&self) -> NonZero<u32> {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        self.duration
    }
}

fn compute_fft_bands(samples: &[f32], bands: usize) -> Vec<f32> {
    if samples.is_empty() || bands == 0 {
        return Vec::new();
    }
    let len = samples.len().next_power_of_two();
    let mut input = vec![Complex::new(0.0f32, 0.0f32); len];
    let offset = samples.len().saturating_sub(len);
    for (idx, sample) in samples.iter().skip(offset).take(len).enumerate() {
        let window = 0.5 - 0.5 * ((2.0 * std::f32::consts::PI * idx as f32) / len as f32).cos();
        input[idx].re = sample * window;
    }

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(len);
    fft.process(&mut input);

    let usable = (len / 2).max(1);
    let mut out = Vec::with_capacity(bands);
    for band in 0..bands {
        let start = 1 + band * usable / bands;
        let end = 1 + (band + 1) * usable / bands;
        let count = end.saturating_sub(start).max(1);
        let energy = input
            .iter()
            .take(end.min(input.len()))
            .skip(start.min(input.len()))
            .map(|c| c.norm())
            .sum::<f32>()
            / count as f32;
        out.push((energy * 2.5).sqrt().min(1.0));
    }
    out
}

struct TrackerSource {
    player: Arc<Mutex<XmrsPlayer<'static>>>,
    visualizer: Arc<Mutex<TrackerVisualizer>>,
    buffer: [f32; BUFFER_SIZE],
    buffer_index: usize,
    buffer_len: usize,
    sample_rate: NonZero<u32>,
    duration: Option<Duration>,
    finished: bool,
}

impl TrackerSource {
    fn new(
        player: Arc<Mutex<XmrsPlayer<'static>>>,
        visualizer: Arc<Mutex<TrackerVisualizer>>,
        duration: Option<Duration>,
    ) -> Self {
        Self {
            player,
            visualizer,
            buffer: [0.0; BUFFER_SIZE],
            buffer_index: BUFFER_SIZE,
            buffer_len: 0,
            sample_rate: NonZero::new(SAMPLE_RATE).expect("sample rate must be non-zero"),
            duration,
            finished: false,
        }
    }

    fn generate_samples(&mut self) {
        if self.finished {
            self.buffer_len = 0;
            if let Ok(mut visualizer) = self.visualizer.lock() {
                visualizer.stop();
            }
            return;
        }

        let frames = self.buffer.len() / 2;
        let mut mono = Vec::with_capacity(frames);
        let (mut table_index, mut pattern, mut row) = (0, 0, 0);
        let mut out_idx = 0usize;
        if let Ok(mut player) = self.player.lock() {
            for idx in 0..frames {
                let Some(sample) = player.sample(true) else {
                    self.finished = true;
                    break;
                };
                let left = sample.0 as f32 / i16::MAX as f32;
                let right = sample.1 as f32 / i16::MAX as f32;
                out_idx = idx * 2;
                self.buffer[out_idx] = left;
                self.buffer[out_idx + 1] = right;
                out_idx += 2;
                mono.push((left + right) * 0.5);
            }
            table_index = player.get_current_table_index();
            pattern = player.playing_pattern();
            row = player.playing_row();
        } else {
            self.finished = true;
        }
        self.buffer_len = out_idx;
        if let Ok(mut visualizer) = self.visualizer.lock() {
            if mono.is_empty() {
                visualizer.stop();
            } else {
                visualizer.update(&mono, table_index, pattern, row);
            }
        }
    }
}

impl Iterator for TrackerSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.buffer_index >= self.buffer_len {
            self.generate_samples();
            self.buffer_index = 0;
            if self.buffer_len == 0 {
                return None;
            }
        }
        let sample = self.buffer[self.buffer_index];
        self.buffer_index += 1;
        Some(sample)
    }
}

impl Source for TrackerSource {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.buffer_len.saturating_sub(self.buffer_index))
    }

    fn channels(&self) -> NonZero<u16> {
        NonZero::new(2).expect("stereo channel count must be non-zero")
    }

    fn sample_rate(&self) -> NonZero<u32> {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        self.duration
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn tracker_module_path_accepts_it() {
        assert!(is_tracker_module_path(Path::new("module.it")));
        assert!(!is_tracker_module_path(Path::new("module.sid")));
    }

    #[test]
    fn wav_path_is_audio() {
        assert!(is_audio_path(Path::new("sample.wav")));
    }
}
