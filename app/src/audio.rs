//! The sound devices, for a real call: the default microphone captured
//! and resampled to 16 kHz mono 20 ms frames, the default speaker fed
//! from a ring buffer of decoded frames. Behind the `sound` feature
//! because it links the platform's audio library; without it the desk
//! still messages and pays, and says calls need sound devices.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate, StreamConfig};

use crate::calls::{uk_ring, Audio};
use crate::log;

const TAG: &str = "Audio";
const RATE: u32 = 16_000;
const FRAME: usize = 320;

/// Linear resampling of interleaved samples to mono at `RATE`.
fn to_mono_16k(input: &[f32], channels: usize, in_rate: u32) -> Vec<i16> {
    let frames = input.len() / channels.max(1);
    let mono: Vec<f32> = (0..frames).map(|i| input[i * channels..(i + 1) * channels].iter().sum::<f32>() / channels as f32).collect();
    if in_rate == RATE {
        return mono.iter().map(|v| (v.clamp(-1.0, 1.0) * 32767.0) as i16).collect();
    }
    let ratio = in_rate as f64 / RATE as f64;
    let out_len = ((frames as f64) / ratio) as usize;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let a = pos.floor() as usize;
            let b = (a + 1).min(mono.len().saturating_sub(1));
            let t = (pos - a as f64) as f32;
            let v = mono.get(a).copied().unwrap_or(0.0) * (1.0 - t) + mono.get(b).copied().unwrap_or(0.0) * t;
            (v.clamp(-1.0, 1.0) * 32767.0) as i16
        })
        .collect()
}

/// Mono 16 kHz to the output's rate and channel count.
fn from_mono_16k(input: &[i16], channels: usize, out_rate: u32) -> Vec<f32> {
    let ratio = RATE as f64 / out_rate as f64;
    let out_frames = ((input.len() as f64) / ratio) as usize;
    let mut out = Vec::with_capacity(out_frames * channels);
    for i in 0..out_frames {
        let pos = i as f64 * ratio;
        let a = pos.floor() as usize;
        let b = (a + 1).min(input.len().saturating_sub(1));
        let t = (pos - a as f64) as f32;
        let v = (input.get(a).copied().unwrap_or(0) as f32 * (1.0 - t) + input.get(b).copied().unwrap_or(0) as f32 * t) / 32767.0;
        for _ in 0..channels {
            out.push(v);
        }
    }
    out
}

pub struct DeviceAudio {
    input: Option<cpal::Stream>,
    output: Option<cpal::Stream>,
    /// Decoded frames waiting for the speaker, as mono 16 kHz samples.
    queue: Arc<Mutex<VecDeque<i16>>>,
    bell: Arc<Mutex<Option<(Vec<i16>, usize)>>>,
}

// cpal streams are not Send on every platform; the call machine only
// touches them from behind its own mutex.
unsafe impl Send for DeviceAudio {}

impl DeviceAudio {
    pub fn new() -> DeviceAudio {
        DeviceAudio { input: None, output: None, queue: Arc::new(Mutex::new(VecDeque::new())), bell: Arc::new(Mutex::new(None)) }
    }

    fn open_output(&mut self) -> bool {
        if self.output.is_some() {
            return true;
        }
        let host = cpal::default_host();
        let Some(dev) = host.default_output_device() else {
            log::warn(TAG, "no speaker");
            return false;
        };
        let Ok(cfg) = dev.default_output_config() else { return false };
        let channels = cfg.channels() as usize;
        let rate = cfg.sample_rate().0;
        let queue = self.queue.clone();
        let bell = self.bell.clone();
        let stream = dev.build_output_stream(
            &StreamConfig { channels: cfg.channels(), sample_rate: SampleRate(rate), buffer_size: cpal::BufferSize::Default },
            move |out: &mut [f32], _| {
                let frames = out.len() / channels.max(1);
                let need = ((frames as f64) * RATE as f64 / rate as f64).ceil() as usize + 1;
                let mut mono: Vec<i16> = Vec::with_capacity(need);
                {
                    let mut b = bell.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some((pcm, at)) = b.as_mut() {
                        for _ in 0..need {
                            mono.push(pcm[*at]);
                            *at = (*at + 1) % pcm.len();
                        }
                    }
                }
                if mono.is_empty() {
                    let mut q = queue.lock().unwrap_or_else(|e| e.into_inner());
                    for _ in 0..need {
                        mono.push(q.pop_front().unwrap_or(0));
                    }
                }
                let rendered = from_mono_16k(&mono, channels, rate);
                for (i, v) in out.iter_mut().enumerate() {
                    *v = rendered.get(i).copied().unwrap_or(0.0);
                }
            },
            |e| log::warn(TAG, format!("speaker: {e}")),
            None,
        );
        match stream {
            Ok(s) => {
                let _ = s.play();
                self.output = Some(s);
                true
            }
            Err(e) => {
                log::warn(TAG, format!("speaker: {e}"));
                false
            }
        }
    }
}

impl Default for DeviceAudio {
    fn default() -> Self {
        Self::new()
    }
}

impl Audio for DeviceAudio {
    fn start(&mut self, on_frame: Box<dyn Fn(Vec<u8>) + Send + Sync>) -> bool {
        self.open_output();
        let host = cpal::default_host();
        let Some(dev) = host.default_input_device() else {
            log::warn(TAG, "no microphone");
            return false;
        };
        let Ok(cfg) = dev.default_input_config() else { return false };
        let channels = cfg.channels() as usize;
        let rate = cfg.sample_rate().0;
        let pending: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::new()));
        let on_frame = Arc::new(on_frame);
        let build = |fmt: SampleFormat| -> Result<cpal::Stream, cpal::BuildStreamError> {
            let pending = pending.clone();
            let on_frame = on_frame.clone();
            let config = StreamConfig { channels: cfg.channels(), sample_rate: SampleRate(rate), buffer_size: cpal::BufferSize::Default };
            let push = move |mono: Vec<i16>| {
                let mut p = pending.lock().unwrap_or_else(|e| e.into_inner());
                p.extend(mono);
                while p.len() >= FRAME {
                    let frame: Vec<i16> = p.drain(..FRAME).collect();
                    let mut pcm = Vec::with_capacity(FRAME * 2);
                    for s in frame {
                        pcm.extend_from_slice(&s.to_le_bytes());
                    }
                    on_frame(pcm);
                }
            };
            match fmt {
                SampleFormat::F32 => dev.build_input_stream(&config, move |data: &[f32], _| push(to_mono_16k(data, channels, rate)), |e| log::warn(TAG, format!("microphone: {e}")), None),
                SampleFormat::I16 => dev.build_input_stream(
                    &config,
                    move |data: &[i16], _| {
                        let f: Vec<f32> = data.iter().map(|v| *v as f32 / 32768.0).collect();
                        push(to_mono_16k(&f, channels, rate))
                    },
                    |e| log::warn(TAG, format!("microphone: {e}")),
                    None,
                ),
                other => {
                    log::warn(TAG, format!("microphone sample format {other:?} not handled"));
                    Err(cpal::BuildStreamError::StreamConfigNotSupported)
                }
            }
        };
        match build(cfg.sample_format()) {
            Ok(s) => {
                if let Err(e) = s.play() {
                    log::warn(TAG, format!("microphone: {e}"));
                    return false;
                }
                self.input = Some(s);
                log::info(TAG, format!("microphone at {rate} Hz × {channels}, speaker open"));
                true
            }
            Err(e) => {
                log::warn(TAG, format!("microphone: {e}"));
                false
            }
        }
    }

    fn play(&mut self, pcm: &[u8]) {
        let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        // A second of backlog is latency nobody wants; drop the oldest.
        while q.len() > RATE as usize {
            q.pop_front();
        }
        for ch in pcm.chunks_exact(2) {
            q.push_back(i16::from_le_bytes([ch[0], ch[1]]));
        }
    }

    fn stop(&mut self) {
        self.input = None;
        self.queue.lock().unwrap_or_else(|e| e.into_inner()).clear();
        *self.bell.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    fn ring(&mut self, incoming: bool) {
        if !self.open_output() {
            return;
        }
        let pcm: Vec<i16> = uk_ring(if incoming { 0.6 } else { 0.25 }).chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
        *self.bell.lock().unwrap_or_else(|e| e.into_inner()) = Some((pcm, 0));
    }

    fn quiet(&mut self) {
        *self.bell.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}
