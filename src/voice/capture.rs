use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

pub fn list_input_devices() {
    let host = cpal::default_host();
    if let Some(d) = host.default_input_device() {
        println!("Default: {:?}", d.name().unwrap_or_else(|_| "<unknown>".into()));
    }
    println!("\nAll input devices:");
    if let Ok(devices) = host.input_devices() {
        for d in devices {
            println!("  {:?}", d.name().unwrap_or_else(|_| "<unknown>".into()));
        }
    }
}

fn normalize_device_name(s: &str) -> String {
    s.to_lowercase()
        .replace([' ', '_', '-'], "")
}

pub fn set_pipewire_default_sink(name: &str) -> Result<()> {
    let out = std::process::Command::new("wpctl")
        .args(["status"])
        .output()
        .context("failed to run wpctl status")?;
    let listing = String::from_utf8_lossy(&out.stdout);
    let needle = normalize_device_name(name);
    let mut in_sinks = false;
    let mut id: Option<String> = None;
    for line in listing.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("├─ Sinks:") || trimmed.starts_with("Sinks:") {
            in_sinks = true;
            continue;
        }
        if in_sinks && (trimmed.starts_with("├─") || trimmed.starts_with("└─") || trimmed.is_empty()) {
            in_sinks = false;
            continue;
        }
        if !in_sinks {
            continue;
        }
        if normalize_device_name(trimmed).contains(&needle) {
            let num = trimmed
                .trim_start_matches(|c: char| c == '*' || c == ' ' || c == '│')
                .trim_start()
                .split('.')
                .next()
                .and_then(|s| s.trim().parse::<u32>().ok());
            if let Some(n) = num {
                id = Some(n.to_string());
                break;
            }
        }
    }
    let id = id.with_context(|| format!("no PipeWire sink matching {name:?}"))?;
    let status = std::process::Command::new("wpctl")
        .args(["set-default", &id])
        .status()
        .context("failed to run wpctl set-default")?;
    if !status.success() {
        anyhow::bail!("wpctl set-default {id} failed");
    }
    eprintln!("hive voice: set default sink to id {id} (matched {name:?})");
    Ok(())
}

fn set_pipewire_default_source(name: &str) -> Result<()> {
    let out = std::process::Command::new("wpctl")
        .args(["status"])
        .output()
        .context("failed to run wpctl status")?;
    let listing = String::from_utf8_lossy(&out.stdout);
    let needle = normalize_device_name(name);
    let mut in_sources = false;
    let mut id: Option<String> = None;
    for line in listing.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("├─ Sources:") || trimmed.starts_with("Sources:") {
            in_sources = true;
            continue;
        }
        if in_sources && (trimmed.starts_with("├─") || trimmed.starts_with("└─") || trimmed.is_empty()) {
            in_sources = false;
            continue;
        }
        if !in_sources {
            continue;
        }
        if normalize_device_name(trimmed).contains(&needle) {
            let num = trimmed
                .trim_start_matches(|c: char| c == '*' || c == ' ' || c == '│')
                .trim_start()
                .split('.')
                .next()
                .and_then(|s| s.trim().parse::<u32>().ok());
            if let Some(n) = num {
                id = Some(n.to_string());
                break;
            }
        }
    }
    let id = id.with_context(|| format!("no PipeWire source matching {name:?}"))?;
    let status = std::process::Command::new("wpctl")
        .args(["set-default", &id])
        .status()
        .context("failed to run wpctl set-default")?;
    if !status.success() {
        anyhow::bail!("wpctl set-default {id} failed");
    }
    eprintln!("hive voice: set default source to id {id} (matched {name:?})");
    Ok(())
}

const TARGET_RATE: u32 = 16000;
const FRAME_MS: u32 = 30;
const PREROLL_FRAMES: usize = 5;

pub struct Recorder {
    _stream: cpal::Stream,
    buf: Arc<Mutex<Vec<f32>>>,
    pub sample_rate: u32,
}

impl Recorder {
    pub fn open(device_name: Option<&str>) -> Result<Self> {
        if let Some(name) = device_name {
            set_pipewire_default_source(name)?;
        }
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .context("no default input device")?;
        let supported = device
            .default_input_config()
            .context("could not query default input config")?;
        let sample_rate = supported.sample_rate().0;
        let channels = supported.channels() as usize;
        let format = supported.sample_format();
        let stream_config: cpal::StreamConfig = supported.into();

        let buf: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
        let err_fn = |e| eprintln!("hive voice: cpal stream error: {e}");

        let stream = match format {
            cpal::SampleFormat::F32 => {
                let buf_cb = buf.clone();
                device.build_input_stream(
                    &stream_config,
                    move |data: &[f32], _| append_mono(&buf_cb, data, channels, |s| s),
                    err_fn,
                    None,
                )?
            }
            cpal::SampleFormat::I16 => {
                let buf_cb = buf.clone();
                device.build_input_stream(
                    &stream_config,
                    move |data: &[i16], _| {
                        append_mono(&buf_cb, data, channels, |s| s as f32 / i16::MAX as f32)
                    },
                    err_fn,
                    None,
                )?
            }
            cpal::SampleFormat::U16 => {
                let buf_cb = buf.clone();
                device.build_input_stream(
                    &stream_config,
                    move |data: &[u16], _| {
                        append_mono(&buf_cb, data, channels, |s| (s as f32 - 32768.0) / 32768.0)
                    },
                    err_fn,
                    None,
                )?
            }
            other => anyhow::bail!("unsupported cpal sample format: {other:?}"),
        };

        stream.play().context("failed to start input stream")?;
        Ok(Self {
            _stream: stream,
            buf,
            sample_rate,
        })
    }

    pub fn drain(&self) -> Vec<f32> {
        std::mem::take(&mut *self.buf.lock().unwrap())
    }

    pub fn clear(&self) {
        self.buf.lock().unwrap().clear();
    }
}

#[derive(Debug, Clone, Copy)]
pub struct VadParams {
    pub threshold: f32,
    pub silence_ms: u32,
    pub min_speech_ms: u32,
}

pub fn record_vad_segment(
    rec: &Recorder,
    params: &VadParams,
    should_abort: impl Fn() -> bool,
) -> Result<Option<Vec<f32>>> {
    rec.clear();
    let rate = rec.sample_rate as usize;
    let frame_samples = (rate * FRAME_MS as usize) / 1000;
    let silence_frames_to_end = (params.silence_ms / FRAME_MS).max(1) as usize;
    let min_speech_frames = (params.min_speech_ms / FRAME_MS).max(1) as usize;

    let mut pending: Vec<f32> = Vec::new();
    let mut preroll: VecDeque<Vec<f32>> = VecDeque::with_capacity(PREROLL_FRAMES + 1);
    let mut segment: Vec<f32> = Vec::new();
    let mut in_speech = false;
    let mut speech_frames: usize = 0;
    let mut trailing_silence: usize = 0;

    loop {
        if should_abort() && !in_speech {
            return Ok(None);
        }

        let mut new_samples = rec.drain();
        if new_samples.is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(20));
            continue;
        }
        pending.append(&mut new_samples);

        while pending.len() >= frame_samples {
            let frame: Vec<f32> = pending.drain(..frame_samples).collect();
            let voiced = frame_rms(&frame) > params.threshold;

            if !in_speech {
                if voiced {
                    for f in preroll.drain(..) {
                        segment.extend_from_slice(&f);
                    }
                    segment.extend_from_slice(&frame);
                    in_speech = true;
                    speech_frames = 1;
                    trailing_silence = 0;
                } else {
                    preroll.push_back(frame);
                    if preroll.len() > PREROLL_FRAMES {
                        preroll.pop_front();
                    }
                }
                continue;
            }

            segment.extend_from_slice(&frame);
            speech_frames += 1;
            if voiced {
                trailing_silence = 0;
                continue;
            }
            trailing_silence += 1;
            if trailing_silence < silence_frames_to_end {
                continue;
            }
            if speech_frames >= min_speech_frames {
                return Ok(Some(resample_to_16k(&segment, rec.sample_rate)));
            }
            segment.clear();
            preroll.clear();
            in_speech = false;
            speech_frames = 0;
            trailing_silence = 0;
        }
    }
}

pub fn resample_to_16k(input: &[f32], src_rate: u32) -> Vec<f32> {
    if input.is_empty() || src_rate == TARGET_RATE {
        return input.to_vec();
    }
    let ratio = TARGET_RATE as f32 / src_rate as f32;
    let out_len = (input.len() as f32 * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    let last = input.len() - 1;
    for i in 0..out_len {
        let src_idx = i as f32 / ratio;
        let lo = (src_idx.floor() as usize).min(last);
        let hi = (lo + 1).min(last);
        let frac = src_idx - lo as f32;
        out.push(input[lo] * (1.0 - frac) + input[hi] * frac);
    }
    out
}

fn frame_rms(frame: &[f32]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    let sum: f32 = frame.iter().map(|s| s * s).sum();
    (sum / frame.len() as f32).sqrt()
}

fn append_mono<S: Copy>(
    buf: &Arc<Mutex<Vec<f32>>>,
    data: &[S],
    channels: usize,
    to_f32: impl Fn(S) -> f32,
) {
    let mut b = buf.lock().unwrap();
    if channels <= 1 {
        b.extend(data.iter().copied().map(to_f32));
        return;
    }
    let inv = 1.0 / channels as f32;
    for chunk in data.chunks(channels) {
        let s: f32 = chunk.iter().copied().map(&to_f32).sum::<f32>() * inv;
        b.push(s);
    }
}
