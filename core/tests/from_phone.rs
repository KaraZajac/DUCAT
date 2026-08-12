//! A backup produced on a handset, opened here.
//!
//! Every other test in this suite builds a bundle and reads it back with the
//! same code. This one takes an artifact written by the Android client — Kotlin
//! calling across JNI into a `.so` built for arm64 — and opens it on x86-64 with
//! the library. It is the only test that can catch an endianness assumption, an
//! Argon2 parameter that differs by target, or a UniFFI marshalling detail that
//! silently changes a byte.
#[test]
fn a_backup_written_by_the_phone_opens_here() {
    let path = "/tmp/bprobe/from-phone.ducatbak";
    let Ok(blob) = std::fs::read(path) else {
        eprintln!("no phone backup at {path}; skipping");
        return;
    };
    let b = ducat_core::backup::import(&blob, b"1234567890")
        .expect("the phone's backup must open with the library");

    assert_eq!(b.persona_secret.len(), 32, "persona key survived");
    assert_eq!(b.monero_seed.len(), 64, "spend key is 32 bytes, hex-encoded");
    assert!(
        b.monero_seed.chars().all(|c| c.is_ascii_hexdigit()),
        "spend key is hex: {}",
        b.monero_seed
    );
    // §15.5.1's thresholds ride along, and must survive as the user set them.
    assert_eq!(b.verification, ducat_core::verify::VerificationPolicy::default());
    println!("  persona   {} bytes", b.persona_secret.len());
    println!("  spend key {}…", &b.monero_seed[..16]);
    println!("  restore   {}", b.monero_restore_height);
    println!("  blob      {} bytes", blob.len());
}
