//! Voice memos: a take from the microphone, sent as an attachment the way
//! the phone sends one — body "🎤", a file called "Voice memo", the mime
//! saying which container. Only one take at a time.

use std::sync::Mutex;
use std::time::Duration;

use crate::audio_proc::MemoRecorder;
use crate::{App, Error};

static TAKE: Mutex<Option<MemoRecorder>> = Mutex::new(None);

/// Shorter than this and nothing was said — the phone's 700 ms.
const TOO_SHORT: Duration = Duration::from_millis(700);

impl App {
    /// Starts a take; a take already running is replaced.
    pub fn memo_start(&self) -> Result<(), Error> {
        let rec = MemoRecorder::start().map_err(Error::Refused)?;
        if let Some(old) = TAKE.lock().unwrap().replace(rec) {
            old.cancel();
        }
        Ok(())
    }

    /// How long the running take is, if one is.
    pub fn memo_elapsed(&self) -> Option<Duration> {
        TAKE.lock().unwrap().as_ref().map(|r| r.elapsed())
    }

    pub fn memo_cancel(&self) {
        if let Some(r) = TAKE.lock().unwrap().take() {
            r.cancel();
        }
    }

    /// Ends the take and sends it to `persona_hex`; the length sent.
    pub fn memo_stop_and_send(&self, persona_hex: &str) -> Result<Duration, Error> {
        let rec = TAKE.lock().unwrap().take().ok_or_else(|| Error::Refused("no memo is being recorded".into()))?;
        let (wav, took) = rec.stop();
        if took < TOO_SHORT || wav.len() <= 44 {
            return Err(Error::Refused("nothing was recorded".into()));
        }
        let dir = self.root().join("files").join("memos");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("Voice memo.wav");
        std::fs::write(&path, &wav)?;
        self.send_attachment(persona_hex, &path, Some("🎤"))?;
        let _ = std::fs::remove_file(&path);
        Ok(took)
    }
}
