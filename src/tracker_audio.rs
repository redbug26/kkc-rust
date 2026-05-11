use anyhow::{Context, Result, anyhow};
use rodio::Source;
use rustfft::{FftPlanner, num_complex::Complex};
use std::io::Cursor;
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use xmrs::fixed::units::Amplification;
use xmrs::prelude::*;
use xmrsplayer::prelude::XmrsPlayer;

const SAMPLE_RATE: u32 = 48_000;
const SAMPLE_RATE_FAST: u32 = 128;
const BUFFER_SIZE: usize = 2048;
const SPECTRUM_BANDS: usize = 96;
const MAX_TRACKER_DURATION_SCAN_SAMPLES: u64 = SAMPLE_RATE_FAST as u64 * 60 * 60 * 6;

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
}

#[derive(Debug)]
struct TrackerVisualizer {
    samples: Vec<f32>,
    write_pos: usize,
    rms: f32,
    spectrum: Vec<f32>,
    table_index: usize,
    pattern: usize,
    row: usize,
    playing: bool,
}

impl TrackerVisualizer {
    fn new() -> Self {
        Self {
            samples: vec![0.0; 1024],
            write_pos: 0,
            rms: 0.0,
            spectrum: vec![0.0; SPECTRUM_BANDS],
            table_index: 0,
            pattern: 0,
            row: 0,
            playing: true,
        }
    }

    fn update(&mut self, samples: &[f32], table_index: usize, pattern: usize, row: usize) {
        for &sample in samples {
            self.samples[self.write_pos] = sample;
            self.write_pos = (self.write_pos + 1) % self.samples.len();
        }
        self.table_index = table_index;
        self.pattern = pattern;
        self.row = row;
        self.playing = true;
        self.rms = if samples.is_empty() {
            0.0
        } else {
            (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
        };
        self.spectrum = compute_fft_bands(&self.ordered_samples(), SPECTRUM_BANDS);
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
            position: Duration::ZERO,
            duration: None,
        }
    }

    fn stop(&mut self) {
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
            "flac" => Some("FLAC"),
            "mp3" => Some("MP3"),
            _ => None,
        })
}

pub fn is_audio_path(path: &Path) -> bool {
    is_tracker_module_path(path) || decoded_audio_format(path).is_some()
}

pub fn module_info(bytes: &[u8]) -> Result<TrackerModuleInfo> {
    let module = Module::load(bytes).map_err(|err| anyhow!("Loading tracker module: {err:?}"))?;
    Ok(info_from_module(&module))
}

pub fn audio_info(path: &Path, bytes: &[u8]) -> Result<TrackerModuleInfo> {
    if decoded_audio_format(path).is_some() {
        decoded_audio_info(path, bytes)
    } else {
        module_info(bytes)
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
        snapshot.position = state.player.get_pos();
        snapshot.duration = state.duration;
        snapshot.playing = !state.player.empty();
        snapshot
    })
}

pub fn playback_finished_for_path(path: &Path) -> bool {
    playback_state()
        .lock()
        .ok()
        .and_then(|state| {
            let state = state.as_ref()?;
            (state.path == path && state.player.empty()).then_some(())
        })
        .is_some()
}

pub fn play_audio_file(path: &Path) -> Result<TrackerModuleInfo> {
    let bytes = std::fs::read(path).with_context(|| format!("Reading {}", path.display()))?;
    play_audio_bytes(path.to_path_buf(), &bytes)
}

pub fn play_audio_bytes(path: PathBuf, bytes: &[u8]) -> Result<TrackerModuleInfo> {
    if decoded_audio_format(&path).is_some() {
        play_decoded_audio_bytes(path, bytes)
    } else {
        play_tracker_bytes(path, bytes)
    }
}

fn play_tracker_bytes(path: PathBuf, bytes: &[u8]) -> Result<TrackerModuleInfo> {
    let module = Module::load(bytes).map_err(|err| anyhow!("Loading tracker module: {err:?}"))?;
    let mut info = info_from_module(&module);
    info.duration = tracker_module_duration(&module);

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
        old.player.stop();
    }
    *state = Some(PlaybackState {
        path,
        _stream: stream,
        player: sink,
        visualizer,
        duration: info.duration,
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
        old.player.stop();
    }
    *state = Some(PlaybackState {
        path,
        _stream: stream,
        player: sink,
        visualizer,
        duration: info.duration,
    });

    Ok(info)
}

pub fn stop_module() {
    if let Ok(mut state) = playback_state().lock()
        && let Some(old) = state.take()
    {
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

fn tracker_module_duration(module: &Module) -> Option<Duration> {
    let mut player = XmrsPlayer::new(module, SAMPLE_RATE_FAST, 0);
    player.set_max_loop_count(1);
    let mut samples = 0u64;
    while player.sample(false).is_some() {
        samples += 1;
        if samples > MAX_TRACKER_DURATION_SCAN_SAMPLES {
            return None;
        }
    }
    Some(Duration::from_secs_f64(
        samples as f64 / SAMPLE_RATE_FAST as f64,
    ))
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
}
