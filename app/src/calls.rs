//! Voice calls (§16.21): a door each side opens on the network, the
//! offer and the answer as messages in the thread, and twenty-millisecond
//! Opus frames through the doors while the call lasts. The phone's
//! `Calls.kt`, with the sound devices behind [`Audio`] so the same machine
//! runs against a microphone, a test tone, or nothing at all.
//!
//! The shape, in one breath: the caller opens a door (a private route),
//! sends a kind-14 offer carrying the door and an id, and listens at the
//! door. The callee's poll notices the offer and rings; answering opens a
//! door of its own, knocks at the caller's with an ANSWER frame carrying
//! that door, and records a kind-15 in the thread for the case where the
//! knock is lost. From then on frames cross both ways; BYE ends it; a
//! side that starves renews its door and tells the other with RENEW.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ducat_mobile::callcodec::{call_conceal, call_decode, call_encode, PCM_BYTES};
use ducat_mobile::contacts::CallSend;
use ducat_mobile::node::{node_call_close, node_call_recv, node_call_route, node_call_send, node_call_send_report};
use serde::Serialize;

use crate::contacts::{hex, hex_to_bytes, now_ms, referent, StoredMessage};
use crate::mailbox::Outgoing;
use crate::{log, App, Error};

const TAG: &str = "Calls";
pub const FRAME_MS: u64 = 20;
pub const RING_WINDOW_SECS: u64 = 90;
pub const NO_ANSWER_LINGER_SECS: u64 = 45;
const CALL_SKEW_SECS: u64 = 60;

const CTRL_ANSWER: u8 = 1;
const CTRL_DECLINE: u8 = 2;
const CTRL_BYE: u8 = 3;
const CTRL_RENEW: u8 = 4;

pub const OFFER_BODY: &str = "📞 Calling";
pub const ANSWER_BODY: &str = "📞 Answered";
pub const DECLINE_BODY: &str = "Call declined";
pub const CANCEL_BODY: &str = "Call cancelled";

/// What a call needs from the machine: a microphone that hands over
/// 640-byte PCM frames (16 kHz, mono, 20 ms) and a speaker that takes
/// them back. `ring`/`quiet` are the bell.
pub trait Audio: Send {
    /// Start capture; `on_frame` is called from the capture thread with
    /// each PCM frame. False when there is no microphone.
    fn start(&mut self, on_frame: Box<dyn Fn(Vec<u8>) + Send + Sync>) -> bool;
    fn play(&mut self, pcm: &[u8]);
    fn stop(&mut self);
    fn ring(&mut self, _incoming: bool) {}
    fn quiet(&mut self) {}
}

/// Three seconds of the UK ring cadence, 16 kHz mono PCM.
pub fn uk_ring(amplitude: f64) -> Vec<u8> {
    let sr = 16_000usize;
    let mut out = vec![0u8; 3 * sr * 2];
    let bursts = [(0.0, 0.4), (0.6, 1.0)];
    for i in 0..3 * sr {
        let t = i as f64 / sr as f64;
        let mut env = 0.0;
        for (a, b) in bursts {
            if t >= a && t < b {
                let edge = (t - a).min(b - t);
                env = if edge >= 0.005 { 1.0 } else { (1.0 - (std::f64::consts::PI * edge / 0.005).cos()) / 2.0 };
            }
        }
        if env == 0.0 {
            continue;
        }
        let tone = (2.0 * std::f64::consts::PI * 400.0 * t).sin() + (2.0 * std::f64::consts::PI * 450.0 * t).sin();
        let v = (tone * 0.5 * env * amplitude * 32767.0) as i32;
        out[i * 2] = (v & 0xff) as u8;
        out[i * 2 + 1] = ((v >> 8) & 0xff) as u8;
    }
    out
}

/// A test microphone: a steady tone, one frame every twenty milliseconds
/// on its own thread; a test speaker that counts what it was handed.
pub struct ToneAudio {
    hz: f64,
    running: Arc<AtomicBool>,
    pub played: Arc<AtomicU64>,
    pub rang: Arc<AtomicBool>,
}

impl ToneAudio {
    pub fn new(hz: f64) -> ToneAudio {
        ToneAudio { hz, running: Arc::new(AtomicBool::new(false)), played: Arc::new(AtomicU64::new(0)), rang: Arc::new(AtomicBool::new(false)) }
    }
}

impl Audio for ToneAudio {
    fn start(&mut self, on_frame: Box<dyn Fn(Vec<u8>) + Send + Sync>) -> bool {
        self.running.store(true, Ordering::SeqCst);
        let running = self.running.clone();
        let hz = self.hz;
        std::thread::Builder::new()
            .name("tone-mic".into())
            .spawn(move || {
                let mut phase = 0.0f64;
                let mut next = Instant::now();
                while running.load(Ordering::SeqCst) {
                    let mut pcm = vec![0u8; PCM_BYTES];
                    for i in 0..PCM_BYTES / 2 {
                        let v = ((phase * 2.0 * std::f64::consts::PI).sin() * 0.3 * 32767.0) as i16;
                        pcm[i * 2..i * 2 + 2].copy_from_slice(&v.to_le_bytes());
                        phase += hz / 16_000.0;
                        if phase >= 1.0 {
                            phase -= 1.0;
                        }
                    }
                    on_frame(pcm);
                    next += Duration::from_millis(FRAME_MS);
                    if let Some(d) = next.checked_duration_since(Instant::now()) {
                        std::thread::sleep(d);
                    }
                }
            })
            .ok();
        true
    }

    fn play(&mut self, _pcm: &[u8]) {
        self.played.fetch_add(1, Ordering::Relaxed);
    }

    fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
    }

    fn ring(&mut self, _incoming: bool) {
        self.rang.store(true, Ordering::SeqCst);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Why {
    RangOut,
    Unreached,
    NeverConnected,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind")]
pub enum CallState {
    Idle,
    Outgoing { contact_hex: String },
    NoAnswer { contact_hex: String, why: Why },
    Incoming { contact_hex: String, offer_seq: u64, call_id: String },
    Answering { contact_hex: String, offer_seq: u64, call_id: String, door: String },
    Active { contact_hex: String, since_ms: u64 },
}

impl CallState {
    pub fn contact_hex(&self) -> Option<&str> {
        match self {
            CallState::Idle => None,
            CallState::Outgoing { contact_hex } | CallState::NoAnswer { contact_hex, .. } | CallState::Incoming { contact_hex, .. } | CallState::Answering { contact_hex, .. } | CallState::Active { contact_hex, .. } => Some(contact_hex),
        }
    }
}

struct Inner {
    state: CallState,
    audio: Option<Box<dyn Audio>>,
    my_call_id: Option<Vec<u8>>,
    their_route: Option<Vec<u8>>,
    dealt_with: std::collections::HashSet<String>,
    reopening: bool,
}

/// One call at a time per process; the phone has the same rule.
pub struct Calls {
    inner: Mutex<Inner>,
    running: AtomicBool,
    epoch: AtomicU32,
    pub rx_frames: AtomicU64,
    pub tx_frames: AtomicU64,
}

static CALLS: std::sync::OnceLock<Calls> = std::sync::OnceLock::new();

pub fn calls() -> &'static Calls {
    CALLS.get_or_init(|| Calls {
        inner: Mutex::new(Inner { state: CallState::Idle, audio: None, my_call_id: None, their_route: None, dealt_with: Default::default(), reopening: false }),
        running: AtomicBool::new(false),
        epoch: AtomicU32::new(0),
        rx_frames: AtomicU64::new(0),
        tx_frames: AtomicU64::new(0),
    })
}

fn control_frame(kind: u8, id: &[u8], route: Option<&[u8]>) -> Vec<u8> {
    let mut out = vec![0u8; 8 + 1 + 8 + route.map_or(0, |r| r.len())];
    out[..4].copy_from_slice(&[0xff; 4]);
    out[8] = kind;
    out[9..17].copy_from_slice(&id[..8]);
    if let Some(r) = route {
        out[17..].copy_from_slice(r);
    }
    out
}

fn is_control(f: &[u8]) -> bool {
    f.len() >= 17 && f[..4] == [0xff; 4]
}

impl Calls {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn state(&self) -> CallState {
        self.lock().state.clone()
    }

    /// The sound devices this process calls with. None until set, which
    /// means calls cannot be placed or answered — the screen says so.
    pub fn set_audio(&self, audio: Box<dyn Audio>) {
        self.lock().audio = Some(audio);
    }

    pub fn has_audio(&self) -> bool {
        self.lock().audio.is_some()
    }

    fn set_state(&self, s: CallState) {
        self.lock().state = s;
        crate::contacts::bump();
    }
}

impl App {
    /// Ring somebody: open a door, send the offer, listen at the door.
    pub fn place_call(&self, persona_hex: &str) -> Result<(), Error> {
        let c = self.contact(persona_hex).ok_or_else(|| Error::Refused("no such contact".into()))?;
        let cs = calls();
        {
            let mut g = cs.lock();
            if g.state != CallState::Idle {
                return Err(Error::Refused("a call is already up".into()));
            }
            if g.audio.is_none() {
                return Err(Error::Refused("no sound devices — calls need a microphone and a speaker".into()));
            }
            let id = ducat_mobile::create_persona_secret()[..8].to_vec();
            g.my_call_id = Some(id);
            g.state = CallState::Outgoing { contact_hex: persona_hex.to_string() };
            if let Some(a) = g.audio.as_mut() {
                a.ring(false);
            }
        }
        crate::contacts::bump();
        let st = CallState::Outgoing { contact_hex: persona_hex.to_string() };
        let ep = cs.epoch.fetch_add(1, Ordering::SeqCst) + 1;
        let app = self.clone();
        let st1 = st.clone();
        let c1 = c.clone();
        std::thread::Builder::new()
            .name("call-place".into())
            .spawn(move || {
                let r: Result<(), Error> = (|| {
                    let mine = node_call_route()?;
                    if calls().state() != st1 {
                        if calls().epoch.load(Ordering::SeqCst) == ep {
                            node_call_close();
                        }
                        return Ok(());
                    }
                    let id = calls().lock().my_call_id.clone().unwrap_or_default();
                    log::info(TAG, format!("ringing with id={}", hex(&id)));
                    app.send(&c1, Outgoing { body: OFFER_BODY.into(), kind: 14, call: Some(CallSend { route: Some(mine), id: Some(id) }), ..Default::default() })?;
                    Ok(())
                })();
                if let Err(e) = r {
                    log::warn(TAG, format!("place: {e}"));
                    if calls().state() == st1 {
                        app.end_call_internal(true, true, false);
                    }
                }
            })
            .ok();
        let app2 = self.clone();
        let st2 = st.clone();
        let c2 = c.clone();
        std::thread::Builder::new()
            .name("call-door".into())
            .spawn(move || {
                let on = || {
                    let s = calls().state();
                    s == st2 || matches!(&s, CallState::NoAnswer { contact_hex, why } if *contact_hex == c2.persona_hex && *why == Why::RangOut)
                };
                while on() {
                    let mut f = node_call_recv(0);
                    while let Some(frame) = f.take() {
                        if calls().state() != st2 {
                            break;
                        }
                        let my_id = calls().lock().my_call_id.clone();
                        if is_control(&frame) && my_id.as_deref().map_or(false, |id| frame[9..17] == id[..8]) {
                            match frame[8] {
                                CTRL_ANSWER if frame.len() > 17 => {
                                    log::info(TAG, "answered at the door");
                                    app2.go_active(&st2, &c2.persona_hex, frame[17..].to_vec(), true, 10_000);
                                }
                                CTRL_DECLINE => {
                                    log::info(TAG, "declined at the door");
                                    app2.end_call_internal(true, false, false);
                                }
                                _ => {}
                            }
                        }
                        if calls().state() != st2 {
                            break;
                        }
                        f = node_call_recv(0);
                    }
                    if !on() {
                        break;
                    }
                    app2.poll_contact(&c2.persona_hex);
                    app2.calls_noticed();
                    std::thread::sleep(Duration::from_secs(2));
                }
            })
            .ok();
        self.expire_ring(st, RING_WINDOW_SECS);
        Ok(())
    }

    fn expire_ring(&self, ringing: CallState, after_secs: u64) {
        let app = self.clone();
        std::thread::Builder::new()
            .name("call-ring-window".into())
            .spawn(move || {
                std::thread::sleep(Duration::from_secs(after_secs));
                if calls().state() == ringing {
                    log::info(TAG, "ring window over");
                    match &ringing {
                        CallState::Incoming { .. } => app.stop_ringing(&ringing),
                        _ => app.end_call_internal(true, false, false),
                    }
                }
            })
            .ok();
    }

    /// Pick up the call that is ringing.
    pub fn answer_call(&self) -> Result<(), Error> {
        let (contact_hex, offer_seq, call_id) = match calls().state() {
            CallState::Incoming { contact_hex, offer_seq, call_id } => (contact_hex, offer_seq, call_id),
            _ => return Err(Error::Refused("nothing is ringing".into())),
        };
        let c = self.contact(&contact_hex).ok_or_else(|| Error::Refused("that contact is gone".into()))?;
        let offer = self.thread(&contact_hex).into_iter().find(|m| !m.outgoing && m.kind == 14 && m.call_id.as_deref() == Some(&call_id) && m.seq == offer_seq).ok_or_else(|| Error::Refused("the offer is gone".into()))?;
        let door_hex = offer.call_route.clone().ok_or_else(|| Error::Refused("the offer names no door".into()))?;
        {
            let mut g = calls().lock();
            if g.audio.is_none() {
                return Err(Error::Refused("no sound devices — calls need a microphone and a speaker".into()));
            }
            if let Some(a) = g.audio.as_mut() {
                a.quiet();
            }
            g.dealt_with.insert(call_id.clone());
            g.state = CallState::Answering { contact_hex: contact_hex.clone(), offer_seq, call_id: call_id.clone(), door: door_hex.clone() };
        }
        crate::contacts::bump();
        let st = CallState::Answering { contact_hex: contact_hex.clone(), offer_seq, call_id: call_id.clone(), door: door_hex.clone() };
        let ep = calls().epoch.fetch_add(1, Ordering::SeqCst) + 1;
        let app = self.clone();
        let patience = patience_for(&offer);
        std::thread::Builder::new()
            .name("call-answer".into())
            .spawn(move || {
                let r: Result<(), Error> = (|| {
                    let mine = node_call_route()?;
                    if calls().state() != st {
                        if calls().epoch.load(Ordering::SeqCst) == ep {
                            node_call_close();
                        }
                        return Ok(());
                    }
                    let id = hex_to_bytes(&call_id).ok_or_else(|| Error::Refused("bad call id".into()))?;
                    calls().lock().my_call_id = Some(id.clone());
                    let door = hex_to_bytes(&door_hex).ok_or_else(|| Error::Refused("bad door".into()))?;
                    for _ in 0..2 {
                        let _ = node_call_send(door.clone(), control_frame(CTRL_ANSWER, &id, Some(&mine)));
                    }
                    if !app.go_active(&st, &contact_hex, door, false, patience) {
                        if calls().epoch.load(Ordering::SeqCst) == ep {
                            node_call_close();
                        }
                        return Ok(());
                    }
                    if let Err(e) = app.send(&c, Outgoing { body: ANSWER_BODY.into(), kind: 15, call: Some(CallSend { route: Some(mine), id: Some(id) }), ..Default::default() }) {
                        log::warn(TAG, format!("answer record: {e}"));
                    }
                    Ok(())
                })();
                if let Err(e) = r {
                    log::warn(TAG, format!("answer: {e}"));
                    if calls().state() == st {
                        app.end_call_internal(false, false, true);
                    }
                }
            })
            .ok();
        Ok(())
    }

    pub fn dismiss_call(&self) {
        let mut g = calls().lock();
        if matches!(g.state, CallState::NoAnswer { .. }) {
            g.my_call_id = None;
            g.state = CallState::Idle;
            drop(g);
            crate::contacts::bump();
        }
    }

    /// Refuse the call that is ringing.
    pub fn decline_call(&self) -> Result<(), Error> {
        let (contact_hex, offer_seq, call_id) = match calls().state() {
            CallState::Incoming { contact_hex, offer_seq, call_id } => (contact_hex, offer_seq, call_id),
            _ => return Err(Error::Refused("nothing is ringing".into())),
        };
        let offer = self.thread(&contact_hex).into_iter().find(|m| !m.outgoing && m.kind == 14 && m.call_id.as_deref() == Some(&call_id));
        {
            let mut g = calls().lock();
            if let Some(a) = g.audio.as_mut() {
                a.quiet();
            }
            g.dealt_with.insert(call_id.clone());
            g.state = CallState::Idle;
        }
        crate::contacts::bump();
        let app = self.clone();
        std::thread::Builder::new()
            .name("call-decline".into())
            .spawn(move || app.refuse(&contact_hex, offer_seq, Some(&call_id), offer.and_then(|o| o.call_route).as_deref()))
            .ok();
        Ok(())
    }

    fn refuse(&self, contact_hex: &str, offer_seq: u64, id_hex: Option<&str>, door_hex: Option<&str>) {
        if let (Some(door), Some(id)) = (door_hex.and_then(hex_to_bytes), id_hex.and_then(hex_to_bytes)) {
            for _ in 0..2 {
                let _ = node_call_send(door.clone(), control_frame(CTRL_DECLINE, &id, None));
            }
        }
        let Some(c) = self.contact(contact_hex) else { return };
        if let Err(e) = self.send(&c, Outgoing { body: DECLINE_BODY.into(), kind: 5, re_seq: Some(offer_seq), re_own: false, ..Default::default() }) {
            log::warn(TAG, format!("decline: {e}"));
        }
    }

    pub fn hang_up(&self) {
        self.end_call_internal(false, false, false);
    }

    /// What the threads say about calls: an answer to our offer, a
    /// decline, an offer ringing for us, the caller giving up. Called
    /// after every poll.
    pub fn calls_noticed(&self) {
        let now = App::now();
        match calls().state() {
            CallState::Outgoing { contact_hex } => {
                let thread = self.thread(&contact_hex);
                let my_id = calls().lock().my_call_id.clone().map(|i| hex(&i));
                let answer = thread.iter().rev().find(|m| !m.outgoing && m.kind == 15 && m.call_id.is_some() && m.call_id == my_id && now.saturating_sub(m.timestamp) < RING_WINDOW_SECS * 2 + CALL_SKEW_SECS);
                if let Some(a) = answer.and_then(|a| a.call_route.as_deref().and_then(hex_to_bytes).map(|r| (a.seq, r))) {
                    log::info(TAG, format!("answered: seq={}", a.0));
                    self.go_active(&CallState::Outgoing { contact_hex: contact_hex.clone() }, &contact_hex, a.1, true, 10_000);
                    return;
                }
                if let Some(offer) = thread.iter().rev().find(|m| m.outgoing && m.kind == 14 && m.call_id == my_id) {
                    if thread.iter().any(|m| !m.outgoing && m.kind == 5 && !m.re_own && referent(&thread, m).map_or(false, |r| std::ptr::eq(r, offer))) {
                        log::info(TAG, "declined");
                        self.end_call_internal(true, false, false);
                    }
                }
            }
            CallState::NoAnswer { contact_hex, why } => {
                let Some(id) = calls().lock().my_call_id.clone() else { return };
                if why != Why::RangOut {
                    return;
                }
                let id_hex = hex(&id);
                let answer = self.thread(&contact_hex).into_iter().rev().find(|m| !m.outgoing && m.kind == 15 && m.call_id.as_deref() == Some(&id_hex) && now.saturating_sub(m.timestamp) < RING_WINDOW_SECS * 2 + CALL_SKEW_SECS);
                if let Some(route) = answer.and_then(|a| a.call_route.as_deref().and_then(hex_to_bytes)) {
                    self.reopen(&CallState::NoAnswer { contact_hex: contact_hex.clone(), why }, route, id);
                }
            }
            CallState::Incoming { contact_hex, call_id, .. } => {
                let thread = self.thread(&contact_hex);
                if let Some(offer) = thread.iter().rev().find(|m| !m.outgoing && m.kind == 14 && m.call_id.as_deref() == Some(&call_id)) {
                    if thread.iter().any(|m| !m.outgoing && m.kind == 5 && m.re_own && referent(&thread, m).map_or(false, |r| std::ptr::eq(r, offer))) {
                        log::info(TAG, "caller hung up before the answer");
                        self.stop_ringing(&calls().state());
                    }
                }
            }
            CallState::Idle => {
                for c in self.contacts() {
                    let thread = self.thread(&c.persona_hex);
                    let dealt = calls().lock().dealt_with.clone();
                    let offer = thread.iter().rev().find(|m| {
                        !m.outgoing
                            && m.kind == 14
                            && now.saturating_sub(m.timestamp) < RING_WINDOW_SECS + CALL_SKEW_SECS
                            && m.call_route.is_some()
                            && m.call_id.as_deref().map_or(false, |id| !dealt.contains(id))
                            && !thread.iter().any(|r| (r.outgoing && r.kind == 15 && r.call_id == m.call_id) || (r.kind == 5 && referent(&thread, r).map_or(false, |x| std::ptr::eq(x, *m))))
                    });
                    if let Some(offer) = offer {
                        if self.start_ringing(&c.persona_hex, offer, now) {
                            return;
                        }
                    }
                }
            }
            CallState::Answering { .. } | CallState::Active { .. } => {}
        }
    }

    fn start_ringing(&self, contact_hex: &str, offer: &StoredMessage, now: u64) -> bool {
        let Some(call_id) = offer.call_id.clone() else { return false };
        {
            let mut g = calls().lock();
            if g.state != CallState::Idle {
                return false;
            }
            g.state = CallState::Incoming { contact_hex: contact_hex.to_string(), offer_seq: offer.seq, call_id };
            if let Some(a) = g.audio.as_mut() {
                a.ring(true);
            }
        }
        crate::contacts::bump();
        let who = self.contact(contact_hex).map(|c| c.display_name()).unwrap_or_default();
        log::info(TAG, format!("ringing: {who} is calling"));
        crate::notify::post("Incoming call", format!("{who} is calling"), Some(contact_hex.to_string()));
        let left = (RING_WINDOW_SECS + CALL_SKEW_SECS).saturating_sub(now.saturating_sub(offer.timestamp)).clamp(1, RING_WINDOW_SECS);
        self.expire_ring(calls().state(), left);
        true
    }

    fn stop_ringing(&self, ringing: &CallState) {
        let mut g = calls().lock();
        if g.state != *ringing {
            return;
        }
        if let Some(a) = g.audio.as_mut() {
            a.quiet();
        }
        if let CallState::Incoming { call_id, .. } = ringing {
            g.dealt_with.insert(call_id.clone());
        }
        g.state = CallState::Idle;
        drop(g);
        crate::contacts::bump();
    }

    fn reopen(&self, from: &CallState, route: Vec<u8>, id: Vec<u8>) {
        {
            let mut g = calls().lock();
            if g.state != *from || g.reopening {
                return;
            }
            g.reopening = true;
        }
        let ep = calls().epoch.load(Ordering::SeqCst);
        let app = self.clone();
        let from = from.clone();
        std::thread::Builder::new()
            .name("call-reopen".into())
            .spawn(move || {
                let r: Result<(), Error> = (|| {
                    let fresh = node_call_route()?;
                    if calls().state() != from {
                        if calls().epoch.load(Ordering::SeqCst) == ep {
                            node_call_close();
                        }
                        return Ok(());
                    }
                    log::info(TAG, format!("answered late (id={}) — reopening our door", hex(&id)));
                    for _ in 0..3 {
                        let _ = node_call_send(route.clone(), control_frame(CTRL_RENEW, &id, Some(&fresh)));
                    }
                    let hex_c = from.contact_hex().unwrap_or_default().to_string();
                    app.go_active(&from, &hex_c, route.clone(), true, 10_000);
                    Ok(())
                })();
                if let Err(e) = r {
                    log::warn(TAG, format!("reopen: {e}"));
                    calls().lock().reopening = false;
                }
            })
            .ok();
    }

    /// The call is on: their door is known, the microphone runs, the pump
    /// reads frames and plays them. False if the state moved or there is
    /// no microphone.
    fn go_active(&self, from: &CallState, contact_hex: &str, route: Vec<u8>, initiator: bool, patience_ms: u64) -> bool {
        let cs = calls();
        {
            let mut g = cs.lock();
            if g.state != *from {
                return false;
            }
            if let Some(a) = g.audio.as_mut() {
                a.quiet();
            }
            g.their_route = Some(route);
            g.reopening = false;
            cs.rx_frames.store(0, Ordering::SeqCst);
            cs.tx_frames.store(0, Ordering::SeqCst);
            cs.running.store(true, Ordering::SeqCst);
            if initiator {
                while node_call_recv(0).is_some() {}
            }
            g.state = CallState::Active { contact_hex: contact_hex.to_string(), since_ms: now_ms() };
        }
        crate::contacts::bump();
        let who = self.contact(contact_hex).map(|c| c.display_name()).unwrap_or_default();
        log::info(TAG, format!("connected with {who}"));
        let seq = Arc::new(AtomicU32::new(0));
        let t0 = Instant::now();
        let started = {
            let mut g = cs.lock();
            let Some(audio) = g.audio.as_mut() else { return false };
            let seq = seq.clone();
            audio.start(Box::new(move |frame: Vec<u8>| {
                let calls = calls();
                if !calls.running.load(Ordering::SeqCst) {
                    return;
                }
                if !initiator && calls.rx_frames.load(Ordering::SeqCst) == 0 {
                    return;
                }
                let Some(door) = calls.lock().their_route.clone() else { return };
                let Ok(pkt) = call_encode(frame) else { return };
                let n = seq.fetch_add(1, Ordering::SeqCst);
                let ms = t0.elapsed().as_millis() as u32;
                let mut out = Vec::with_capacity(8 + pkt.len());
                out.extend_from_slice(&n.to_be_bytes());
                out.extend_from_slice(&ms.to_be_bytes());
                out.extend_from_slice(&pkt);
                if node_call_send(door, out).is_ok() {
                    calls.tx_frames.fetch_add(1, Ordering::Relaxed);
                }
            }))
        };
        if !started {
            log::warn(TAG, "no microphone — ending the call");
            self.end_call_internal(false, false, false);
            return false;
        }
        let app = self.clone();
        std::thread::Builder::new()
            .name("call-rx".into())
            .spawn(move || {
                let calls = calls();
                let mut last_heard = Instant::now();
                let mut last_seq: i64 = -1;
                let mut win_start = Instant::now();
                let mut win_count = 0u32;
                let mut last_renew = Instant::now() - Duration::from_secs(60);
                while calls.running.load(Ordering::SeqCst) {
                    let f = node_call_recv(50);
                    if win_start.elapsed() >= Duration::from_secs(6) {
                        if calls.rx_frames.load(Ordering::SeqCst) > 100 && win_count < 90 && last_renew.elapsed() > Duration::from_secs(15) && calls.running.load(Ordering::SeqCst) {
                            last_renew = Instant::now();
                            let (id, door) = {
                                let g = calls.lock();
                                (g.my_call_id.clone(), g.their_route.clone())
                            };
                            if let (Some(id), Some(door)) = (id, door) {
                                let per_s = win_count / 6;
                                std::thread::spawn(move || {
                                    if let Ok(fresh) = node_call_route() {
                                        log::info(TAG, format!("starving ({per_s}/s) — renewing our door"));
                                        for _ in 0..3 {
                                            let _ = node_call_send(door.clone(), control_frame(CTRL_RENEW, &id, Some(&fresh)));
                                        }
                                    }
                                });
                            }
                        }
                        win_start = Instant::now();
                        win_count = 0;
                    }
                    match f {
                        Some(f) if is_control(&f) => {
                            last_heard = Instant::now();
                            let mine = calls.lock().my_call_id.clone();
                            if mine.as_deref().map_or(false, |id| f[9..17] == id[..8]) {
                                match f[8] {
                                    CTRL_BYE => {
                                        log::info(TAG, "BYE — they hung up");
                                        app.end_call_internal(false, false, false);
                                    }
                                    CTRL_RENEW if f.len() > 17 => {
                                        calls.lock().their_route = Some(f[17..].to_vec());
                                        log::info(TAG, "re-aimed at their new door");
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Some(f) if f.len() > 8 => {
                            win_count += 1;
                            last_heard = Instant::now();
                            let seq = u32::from_be_bytes([f[0], f[1], f[2], f[3]]) as i64;
                            if seq <= last_seq {
                                continue;
                            }
                            let gap = seq - last_seq - 1;
                            if last_seq >= 0 && (1..=5).contains(&gap) {
                                for _ in 0..gap {
                                    if let Ok(pcm) = call_conceal() {
                                        if let Some(a) = calls.lock().audio.as_mut() {
                                            a.play(&pcm);
                                        }
                                    }
                                }
                            }
                            last_seq = seq;
                            calls.rx_frames.fetch_add(1, Ordering::Relaxed);
                            if let Ok(pcm) = call_decode(f[8..].to_vec()) {
                                if let Some(a) = calls.lock().audio.as_mut() {
                                    a.play(&pcm);
                                }
                            }
                        }
                        _ => {
                            let heard_any = calls.rx_frames.load(Ordering::SeqCst) > 0;
                            let limit = if heard_any { Duration::from_secs(10) } else { Duration::from_millis(patience_ms) };
                            if last_heard.elapsed() > limit {
                                log::info(TAG, format!("silence — the far side hung up (rx={} tx={})", calls.rx_frames.load(Ordering::SeqCst), calls.tx_frames.load(Ordering::SeqCst)));
                                app.end_call_internal(false, false, !heard_any);
                            }
                        }
                    }
                }
            })
            .ok();
        true
    }

    fn end_call_internal(&self, no_answer: bool, unreached: bool, silent: bool) {
        let cs = calls();
        let (say_bye, route, id, next, refusing, withdraw) = {
            let mut g = cs.lock();
            let ringing_out = matches!(&g.state, CallState::Outgoing { .. });
            let contact = g.state.contact_hex().map(String::from);
            let unanswered = if ringing_out && no_answer { contact.clone() } else { None };
            let unheard = match &g.state {
                CallState::Active { contact_hex, .. } | CallState::Answering { contact_hex, .. } if silent => Some(contact_hex.clone()),
                _ => None,
            };
            let withdraw = if ringing_out && !no_answer { contact.clone() } else { None };
            let refusing = match &g.state {
                CallState::Answering { contact_hex, offer_seq, call_id, door } if !silent => Some((contact_hex.clone(), *offer_seq, call_id.clone(), door.clone())),
                _ => None,
            };
            let say_bye = cs.running.swap(false, Ordering::SeqCst);
            let route = g.their_route.take();
            let id = g.my_call_id.clone();
            log::info(TAG, format!("sends ok/failed: {}", node_call_send_report()));
            if let Some(a) = g.audio.as_mut() {
                a.quiet();
                a.stop();
            }
            g.my_call_id = if unanswered.is_some() && !unreached { g.my_call_id.clone() } else { None };
            g.reopening = false;
            let next = if let Some(h) = unanswered {
                CallState::NoAnswer { contact_hex: h, why: if unreached { Why::Unreached } else { Why::RangOut } }
            } else if let Some(h) = unheard {
                CallState::NoAnswer { contact_hex: h, why: Why::NeverConnected }
            } else {
                CallState::Idle
            };
            g.state = next.clone();
            (say_bye, route, id, next, refusing, withdraw)
        };
        crate::contacts::bump();
        log::info(TAG, format!("call ended → {next:?}"));
        let ep = cs.epoch.load(Ordering::SeqCst);
        let app = self.clone();
        std::thread::Builder::new()
            .name("call-end".into())
            .spawn(move || {
                if say_bye {
                    if let (Some(route), Some(id)) = (&route, &id) {
                        for _ in 0..3 {
                            let _ = node_call_send(route.clone(), control_frame(CTRL_BYE, id, None));
                        }
                        std::thread::sleep(Duration::from_millis(250));
                    }
                }
                if let Some((hex_c, seq, cid, door)) = refusing {
                    app.refuse(&hex_c, seq, Some(&cid), Some(&door));
                    std::thread::sleep(Duration::from_millis(250));
                }
                if let (Some(hex_c), Some(id)) = (withdraw, id) {
                    app.withdraw_offer(&hex_c, &hex(&id));
                }
                if calls().epoch.load(Ordering::SeqCst) == ep {
                    node_call_close();
                }
            })
            .ok();
    }

    /// The caller gave up: a retraction of the offer, once the offer row
    /// exists (a send that has not landed yet is waited for).
    fn withdraw_offer(&self, contact_hex: &str, id_hex: &str) {
        let deadline = Instant::now() + Duration::from_secs(RING_WINDOW_SECS);
        let mut wait = Duration::from_millis(100);
        let offer = loop {
            if let Some(o) = self.thread(contact_hex).into_iter().rev().find(|m| m.outgoing && m.kind == 14 && m.call_id.as_deref() == Some(id_hex)) {
                break Some(o);
            }
            if Instant::now() >= deadline {
                break None;
            }
            std::thread::sleep(wait.min(deadline - Instant::now()));
            wait = (wait * 2).min(Duration::from_secs(5));
        };
        let (Some(offer), Some(c)) = (offer, self.contact(contact_hex)) else { return };
        match self.send(&c, Outgoing { body: CANCEL_BODY.into(), kind: 5, re_seq: Some(offer.seq), re_own: true, ..Default::default() }) {
            Ok(_) => log::info(TAG, "withdrew the offer"),
            Err(e) => log::warn(TAG, format!("withdraw: {e}")),
        }
    }
}

fn patience_for(offer: &StoredMessage) -> u64 {
    let theirs = RING_WINDOW_SECS + CALL_SKEW_SECS + NO_ANSWER_LINGER_SECS;
    let until = (offer.timestamp + theirs) * 1000;
    let left = until as i64 - now_ms() as i64 + 10_000;
    (left.max(10_000) as u64).min(theirs * 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_frames_carry_the_sentinel_the_type_and_the_id() {
        let id = [7u8; 8];
        let f = control_frame(CTRL_ANSWER, &id, Some(&[1, 2, 3]));
        assert!(is_control(&f));
        assert_eq!(f[8], CTRL_ANSWER);
        assert_eq!(&f[9..17], &id);
        assert_eq!(&f[17..], &[1, 2, 3]);
        assert!(!is_control(&[0u8; 20]));
        assert!(!is_control(&[0xff; 10]));
    }

    #[test]
    fn the_bell_is_three_seconds_of_two_bursts() {
        let r = uk_ring(0.5);
        assert_eq!(r.len(), 3 * 16_000 * 2);
        // Silence in the gap between the bursts, sound inside one.
        let at = |t: f64| i16::from_le_bytes([r[(t * 16_000.0) as usize * 2], r[(t * 16_000.0) as usize * 2 + 1]]);
        assert_eq!(at(0.5), 0);
        assert!((0..100).any(|i| at(0.2 + i as f64 * 0.001) != 0));
    }

    #[test]
    fn the_tone_microphone_frames_and_stops() {
        let mut mic = ToneAudio::new(440.0);
        let n = Arc::new(AtomicU64::new(0));
        let n2 = n.clone();
        assert!(mic.start(Box::new(move |f| {
            assert_eq!(f.len(), PCM_BYTES);
            n2.fetch_add(1, Ordering::Relaxed);
        })));
        std::thread::sleep(Duration::from_millis(120));
        mic.stop();
        let got = n.load(Ordering::Relaxed);
        assert!((3..=8).contains(&got), "{got} frames in 120 ms");
        let pcm = call_encode(vec![0u8; PCM_BYTES]).unwrap();
        assert!(!pcm.is_empty());
        assert_eq!(call_decode(pcm).unwrap().len(), PCM_BYTES);
    }
}
