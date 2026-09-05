//! Real sound without a sound library to build against. The microphone and
//! the speaker are the desktop's own PipeWire (or PulseAudio) command-line
//! tools, run as child processes carrying raw 16 kHz mono PCM over pipes —
//! exactly the frames the call codec wants, so nothing is resampled here.
//!
//! `DeviceAudio` behind the `sound` feature is the road on every platform
//! once its headers are on the build machine; this one needs nothing but
//! tools a Linux desktop already ships, and is what a desk built without
//! the feature falls back to. Nothing here is Linux-specific by design: any
//! system with a tool that speaks raw PCM on a pipe could be added to `Tool`.

use std::io::{Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ducat_mobile::callcodec::PCM_BYTES;

use crate::calls::{uk_ring, Audio};
use crate::log;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    PipeWire,
    Pulse,
}

impl Tool {
    fn record(self) -> Command {
        let mut c = match self {
            Tool::PipeWire => {
                let mut c = Command::new("pw-record");
                c.args(["--raw", "--rate=16000", "--channels=1", "--format=s16", "--latency=20ms", "-"]);
                c
            }
            Tool::Pulse => {
                let mut c = Command::new("parec");
                c.args(["--raw", "--format=s16le", "--rate=16000", "--channels=1", "--latency-msec=20"]);
                c
            }
        };
        c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
        c
    }

    fn play(self) -> Command {
        let mut c = match self {
            Tool::PipeWire => {
                let mut c = Command::new("pw-play");
                c.args(["--raw", "--rate=16000", "--channels=1", "--format=s16", "--latency=40ms", "-"]);
                c
            }
            Tool::Pulse => {
                let mut c = Command::new("pacat");
                c.args(["--playback", "--raw", "--format=s16le", "--rate=16000", "--channels=1", "--latency-msec=40"]);
                c
            }
        };
        c.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
        c
    }

    pub fn name(self) -> &'static str {
        match self {
            Tool::PipeWire => "PipeWire",
            Tool::Pulse => "PulseAudio",
        }
    }
}

fn on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else { return false };
    std::env::split_paths(&path).any(|d| d.join(name).is_file())
}

/// Which tool this machine has, if any.
pub fn available() -> Option<Tool> {
    if on_path("pw-record") && on_path("pw-play") {
        Some(Tool::PipeWire)
    } else if on_path("parec") && on_path("pacat") {
        Some(Tool::Pulse)
    } else {
        None
    }
}

fn kill(child: &mut Option<Child>) {
    if let Some(mut c) = child.take() {
        let _ = c.kill();
        let _ = c.wait();
    }
}

pub struct ProcessAudio {
    tool: Tool,
    mic: Option<Child>,
    speaker: Option<Child>,
    speaker_in: Option<ChildStdin>,
    running: Arc<AtomicBool>,
    ringing: Arc<AtomicBool>,
    bell: Arc<Mutex<Option<Child>>>,
}

impl ProcessAudio {
    /// Some when a capture tool is on the PATH; nothing is started yet.
    pub fn detect() -> Option<ProcessAudio> {
        let tool = available()?;
        Some(ProcessAudio {
            tool,
            mic: None,
            speaker: None,
            speaker_in: None,
            running: Arc::new(AtomicBool::new(false)),
            ringing: Arc::new(AtomicBool::new(false)),
            bell: Arc::new(Mutex::new(None)),
        })
    }

    pub fn tool(&self) -> Tool {
        self.tool
    }

    fn open_speaker(&mut self) {
        if self.speaker_in.is_some() {
            return;
        }
        match self.tool.play().spawn() {
            Ok(mut c) => {
                self.speaker_in = c.stdin.take();
                self.speaker = Some(c);
            }
            Err(e) => log::warn("Audio", &format!("no speaker: {e}")),
        }
    }
}

impl Audio for ProcessAudio {
    fn start(&mut self, on_frame: Box<dyn Fn(Vec<u8>) + Send + Sync>) -> bool {
        self.stop();
        let mut child = match self.tool.record().spawn() {
            Ok(c) => c,
            Err(e) => {
                log::warn("Audio", &format!("no microphone: {e}"));
                return false;
            }
        };
        let Some(mut out) = child.stdout.take() else { return false };
        self.running.store(true, Ordering::SeqCst);
        let running = self.running.clone();
        let spawned = std::thread::Builder::new()
            .name("mic-pipe".into())
            .spawn(move || {
                let mut buf = vec![0u8; PCM_BYTES];
                while running.load(Ordering::SeqCst) {
                    if out.read_exact(&mut buf).is_err() {
                        break;
                    }
                    on_frame(buf.clone());
                }
            })
            .is_ok();
        if !spawned {
            let _ = child.kill();
            return false;
        }
        self.mic = Some(child);
        self.open_speaker();
        log::info("Audio", &format!("{} carries the call", self.tool.name()));
        true
    }

    fn play(&mut self, pcm: &[u8]) {
        if self.speaker_in.is_none() {
            self.open_speaker();
        }
        let dead = match self.speaker_in.as_mut() {
            Some(w) => w.write_all(pcm).is_err(),
            None => false,
        };
        if dead {
            // The player went away (a device change, a sound server restart);
            // the next frame gets a fresh one.
            self.speaker_in = None;
            kill(&mut self.speaker);
        }
    }

    fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        kill(&mut self.mic);
        self.speaker_in = None;
        kill(&mut self.speaker);
        self.quiet();
    }

    fn ring(&mut self, incoming: bool) {
        if self.ringing.swap(true, Ordering::SeqCst) {
            return;
        }
        let ringing = self.ringing.clone();
        let bell = self.bell.clone();
        let tool = self.tool;
        let tone = uk_ring(if incoming { 0.35 } else { 0.2 });
        std::thread::Builder::new()
            .name("bell".into())
            .spawn(move || {
                let mut child = match tool.play().spawn() {
                    Ok(c) => c,
                    Err(_) => {
                        ringing.store(false, Ordering::SeqCst);
                        return;
                    }
                };
                let Some(mut w) = child.stdin.take() else { return };
                *bell.lock().unwrap() = Some(child);
                'ring: while ringing.load(Ordering::SeqCst) {
                    // Small writes so a hang-up stops the bell within a frame,
                    // not a whole cadence; the pipe's own back-pressure paces us.
                    for chunk in tone.chunks(PCM_BYTES) {
                        if !ringing.load(Ordering::SeqCst) || w.write_all(chunk).is_err() {
                            break 'ring;
                        }
                    }
                }
                drop(w);
                kill(&mut bell.lock().unwrap());
            })
            .ok();
    }

    fn quiet(&mut self) {
        self.ringing.store(false, Ordering::SeqCst);
        kill(&mut self.bell.lock().unwrap());
        // Let the bell thread notice before a new ring could start one more.
        std::thread::sleep(Duration::from_millis(5));
    }
}

impl Drop for ProcessAudio {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_only_names_tools_that_exist() {
        // Whatever the machine has, the answer must be consistent with PATH.
        match available() {
            Some(Tool::PipeWire) => assert!(on_path("pw-record")),
            Some(Tool::Pulse) => assert!(on_path("parec")),
            None => assert!(!(on_path("pw-record") && on_path("pw-play"))),
        }
    }

    #[test]
    fn a_missing_tool_is_reported_not_panicked() {
        let mut a = ProcessAudio {
            tool: Tool::Pulse,
            mic: None,
            speaker: None,
            speaker_in: None,
            running: Arc::new(AtomicBool::new(false)),
            ringing: Arc::new(AtomicBool::new(false)),
            bell: Arc::new(Mutex::new(None)),
        };
        // Force a command that cannot exist by hijacking PATH for this test.
        let old = std::env::var_os("PATH");
        std::env::set_var("PATH", "/nonexistent");
        let started = a.start(Box::new(|_| {}));
        if let Some(p) = old {
            std::env::set_var("PATH", p);
        }
        assert!(!started);
        a.play(&[0u8; PCM_BYTES]);
        a.stop();
    }
}

/// A voice memo being taken: the capture tool's raw PCM gathered until
/// `stop`, then wrapped as a WAV — the phone records AAC in an MP4, the
/// desk has no encoder, and both play either.
pub struct MemoRecorder {
    child: Child,
    buf: Arc<Mutex<Vec<u8>>>,
    started: std::time::Instant,
}

impl MemoRecorder {
    pub fn start() -> Result<MemoRecorder, String> {
        let tool = available().ok_or_else(|| "no sound tools on this machine".to_string())?;
        let mut child = tool.record().spawn().map_err(|e| format!("no microphone: {e}"))?;
        let mut out = child.stdout.take().ok_or_else(|| "no microphone pipe".to_string())?;
        let buf = Arc::new(Mutex::new(Vec::new()));
        let sink = buf.clone();
        std::thread::Builder::new()
            .name("memo".into())
            .spawn(move || {
                let mut chunk = vec![0u8; 4096];
                loop {
                    match out.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => sink.lock().unwrap().extend_from_slice(&chunk[..n]),
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(MemoRecorder { child, buf, started: std::time::Instant::now() })
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Ends the take: the WAV bytes and how long it ran.
    pub fn stop(mut self) -> (Vec<u8>, Duration) {
        let took = self.started.elapsed();
        let _ = self.child.kill();
        let _ = self.child.wait();
        // The reader thread ends on EOF once the child is gone.
        std::thread::sleep(Duration::from_millis(30));
        let pcm = std::mem::take(&mut *self.buf.lock().unwrap());
        (wav_of(&pcm, 16_000, 1), took)
    }

    /// Drops the take.
    pub fn cancel(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A canonical 44-byte header around 16-bit little-endian PCM.
pub fn wav_of(pcm: &[u8], rate: u32, channels: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(pcm.len() + 44);
    let block = channels * 2;
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * block as u32).to_le_bytes());
    out.extend_from_slice(&block.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(pcm);
    out
}

#[cfg(test)]
mod wav_tests {
    use super::*;

    #[test]
    fn a_wav_header_describes_its_pcm() {
        let pcm = vec![0u8; 640];
        let w = wav_of(&pcm, 16_000, 1);
        assert_eq!(w.len(), 684);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()), 36 + 640);
        assert_eq!(&w[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes(w[24..28].try_into().unwrap()), 16_000);
        assert_eq!(u32::from_le_bytes(w[28..32].try_into().unwrap()), 32_000);
        assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()), 640);
    }
}
