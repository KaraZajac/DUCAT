//! §16.21 media codec: Opus over the call routes.
//!
//! One encoder and one decoder — a device holds one call — behind a mutex,
//! built lazily and torn down with the call. Hard CBR on purpose: encrypted
//! variable-bitrate voice famously leaks speech through packet sizes alone
//! (phrase spotting, even phoneme reconstruction), so every 20 ms frame
//! leaves at exactly the same size whether the speaker is talking or
//! holding their breath.
//!
//! `unsafe-libopus` is xiph's libopus translated mechanically to Rust
//! (BSD-3-Clause), which keeps the build pure cargo on every target the
//! phone and the desk compile for — no C toolchain, no cmake.

use std::sync::{Mutex, OnceLock};

use unsafe_libopus::{
    opus_decode, opus_decoder_create, opus_decoder_destroy, opus_encode, opus_encoder_create,
    opus_encoder_ctl_impl, opus_encoder_destroy, varargs, OpusDecoder, OpusEncoder,
    OPUS_APPLICATION_VOIP, OPUS_SET_BITRATE_REQUEST, OPUS_SET_COMPLEXITY_REQUEST,
    OPUS_SET_VBR_REQUEST,
};

use crate::node::NodeError;

/// 16 kHz mono, 20 ms a frame: 320 samples, 640 PCM bytes.
pub const SAMPLE_RATE: i32 = 16_000;
pub const FRAME_SAMPLES: usize = 320;
pub const PCM_BYTES: usize = FRAME_SAMPLES * 2;

/// Hard CBR at 24 kbit/s: exactly 60 bytes per 20 ms packet.
const BITRATE: i32 = 24_000;

/// Voice at 16 kHz is transparent well below max effort; keep phone CPU low.
const COMPLEXITY: i32 = 5;

const MAX_PACKET: usize = 400;

struct Codec {
    enc: *mut OpusEncoder,
    dec: *mut OpusDecoder,
}

// The raw pointers are only ever touched under the mutex.
unsafe impl Send for Codec {}

static CODEC: OnceLock<Mutex<Option<Codec>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<Codec>> {
    CODEC.get_or_init(|| Mutex::new(None))
}

fn ensure(slot: &mut Option<Codec>) -> Result<(), NodeError> {
    if slot.is_some() {
        return Ok(());
    }
    unsafe {
        let mut err = 0i32;
        let enc = opus_encoder_create(SAMPLE_RATE, 1, OPUS_APPLICATION_VOIP, &mut err);
        if err != 0 {
            return Err(NodeError::Failed(format!("opus encoder: {err}")));
        }
        opus_encoder_ctl_impl(enc, OPUS_SET_BITRATE_REQUEST, varargs!(BITRATE));
        opus_encoder_ctl_impl(enc, OPUS_SET_VBR_REQUEST, varargs!(0i32));
        opus_encoder_ctl_impl(enc, OPUS_SET_COMPLEXITY_REQUEST, varargs!(COMPLEXITY));
        let mut derr = 0i32;
        let dec = opus_decoder_create(SAMPLE_RATE, 1, &mut derr);
        if derr != 0 {
            opus_encoder_destroy(enc);
            return Err(NodeError::Failed(format!("opus decoder: {derr}")));
        }
        *slot = Some(Codec { enc, dec });
    }
    Ok(())
}

/// 640 bytes of PCM16LE in, one Opus packet out (60 bytes, always).
#[uniffi::export]
pub fn call_encode(pcm: Vec<u8>) -> Result<Vec<u8>, NodeError> {
    if pcm.len() != PCM_BYTES {
        return Err(NodeError::Failed(format!(
            "pcm frame is {} bytes, not {PCM_BYTES}",
            pcm.len()
        )));
    }
    let mut samples = [0i16; FRAME_SAMPLES];
    for (i, s) in samples.iter_mut().enumerate() {
        *s = i16::from_le_bytes([pcm[i * 2], pcm[i * 2 + 1]]);
    }
    let mut guard = crate::lock(slot());
    ensure(&mut guard)?;
    let codec = guard.as_mut().unwrap();
    let mut out = vec![0u8; MAX_PACKET];
    let n = unsafe {
        opus_encode(
            codec.enc,
            samples.as_ptr(),
            FRAME_SAMPLES as i32,
            out.as_mut_ptr(),
            MAX_PACKET as i32,
        )
    };
    if n < 0 {
        return Err(NodeError::Failed(format!("opus encode: {n}")));
    }
    out.truncate(n as usize);
    Ok(out)
}

/// One Opus packet in, 640 bytes of PCM16LE out.
#[uniffi::export]
pub fn call_decode(packet: Vec<u8>) -> Result<Vec<u8>, NodeError> {
    if packet.is_empty() || packet.len() > MAX_PACKET {
        return Err(NodeError::Failed(format!(
            "opus packet is {} bytes",
            packet.len()
        )));
    }
    let mut guard = crate::lock(slot());
    ensure(&mut guard)?;
    let codec = guard.as_mut().unwrap();
    let mut samples = [0i16; FRAME_SAMPLES];
    let n = unsafe {
        opus_decode(
            codec.dec,
            packet.as_ptr(),
            packet.len() as i32,
            samples.as_mut_ptr(),
            FRAME_SAMPLES as i32,
            0,
        )
    };
    if n != FRAME_SAMPLES as i32 {
        return Err(NodeError::Failed(format!("opus decode: {n}")));
    }
    let mut pcm = vec![0u8; PCM_BYTES];
    for (i, s) in samples.iter().enumerate() {
        let b = s.to_le_bytes();
        pcm[i * 2] = b[0];
        pcm[i * 2 + 1] = b[1];
    }
    Ok(pcm)
}

/// One concealment frame: what Opus guesses the lost 20 ms sounded like,
/// keeping the decoder's state continuous across a gap so the frames
/// after it decode clean instead of smeared.
#[uniffi::export]
pub fn call_conceal() -> Result<Vec<u8>, NodeError> {
    let mut guard = crate::lock(slot());
    ensure(&mut guard)?;
    let codec = guard.as_mut().unwrap();
    let mut samples = [0i16; FRAME_SAMPLES];
    let n = unsafe {
        opus_decode(
            codec.dec,
            std::ptr::null(),
            0,
            samples.as_mut_ptr(),
            FRAME_SAMPLES as i32,
            0,
        )
    };
    if n != FRAME_SAMPLES as i32 {
        return Err(NodeError::Failed(format!("opus conceal: {n}")));
    }
    let mut pcm = vec![0u8; PCM_BYTES];
    for (i, s) in samples.iter().enumerate() {
        let b = s.to_le_bytes();
        pcm[i * 2] = b[0];
        pcm[i * 2 + 1] = b[1];
    }
    Ok(pcm)
}

/// The call is over: codec state dies with it, fresh pair next call.
pub(crate) fn reset() {
    if let Some(c) = crate::lock(slot()).take() {
        unsafe {
            opus_encoder_destroy(c.enc);
            opus_decoder_destroy(c.dec);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone_frame(seq: usize) -> Vec<u8> {
        let mut pcm = vec![0u8; PCM_BYTES];
        for i in 0..FRAME_SAMPLES {
            let t = (seq * FRAME_SAMPLES + i) as f64 / SAMPLE_RATE as f64;
            let v = ((2.0 * std::f64::consts::PI * 440.0 * t).sin() * 12_000.0) as i16;
            let b = v.to_le_bytes();
            pcm[i * 2] = b[0];
            pcm[i * 2 + 1] = b[1];
        }
        pcm
    }

    /// The whole contract in one place: constant packet size (the privacy
    /// property), and a decoded tone that is still recognizably the tone.
    #[test]
    fn cbr_round_trip_keeps_the_tone() {
        reset();
        let mut sizes = std::collections::HashSet::new();
        for seq in 0..50 {
            let pkt = call_encode(tone_frame(seq)).unwrap();
            sizes.insert(pkt.len());
            let pcm = call_decode(pkt).unwrap();
            assert_eq!(pcm.len(), PCM_BYTES);
            if seq < 3 {
                continue; // encoder ramp-in
            }
            let mut zc = 0;
            let mut rms = 0.0f64;
            let mut prev = 0i16;
            for i in 0..FRAME_SAMPLES {
                let v = i16::from_le_bytes([pcm[i * 2], pcm[i * 2 + 1]]);
                rms += (v as f64) * (v as f64);
                if i > 0 && (v >= 0) != (prev >= 0) {
                    zc += 1;
                }
                prev = v;
            }
            let rms = (rms / FRAME_SAMPLES as f64).sqrt();
            // 440 Hz over 20 ms is ~17.6 sign changes; amplitude 12000 is
            // RMS ~8485. Lossy, but it must still be this tone.
            assert!((15..=21).contains(&zc), "seq {seq}: {zc} crossings");
            assert!((5000.0..=11_000.0).contains(&rms), "seq {seq}: rms {rms}");
        }
        assert_eq!(sizes.len(), 1, "CBR wobbled: {sizes:?}");
        assert!(sizes.contains(&60), "24 kbit/s × 20 ms should be 60 B: {sizes:?}");
        reset();
    }
}
