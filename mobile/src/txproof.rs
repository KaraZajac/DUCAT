//! §9.5 — Monero payment proofs, made and checked here rather than by a
//! wallet-rpc nobody carries on a phone.
//!
//! An `OutProofV2` is what a sender produces from a transaction's secret key
//! `r` to show that transaction `txid` paid address `(A, B)`: for each
//! transaction key it carries the shared secret `D = r·A` and a Schnorr-style
//! proof that the same `r` is behind both `R = r·G` (or `r·B` for a
//! subaddress) and `D`, the whole thing bound to a message. A verifier with
//! the transaction re-derives the outputs' one-time keys from `8·D`, finds
//! the ones that belong to the address, and decrypts their amounts. Nothing
//! about the sender leaks beyond "this transaction, this amount, this message".
//!
//! The layout below follows `crypto::generate_tx_proof` / `check_tx_proof`
//! (version 2) and `wallet2::get_tx_proof` / `check_tx_proof` in the
//! reference wallet, byte for byte, and is checked against monero-wallet-rpc's
//! `check_tx_proof` in the harness. The burn address of §9.5 is derived here
//! too, so both clients and any auditor compute the same one.

use curve25519_dalek::constants::ED25519_BASEPOINT_POINT as G;
use curve25519_dalek::edwards::{CompressedEdwardsY, EdwardsPoint};
use curve25519_dalek::scalar::Scalar as DScalar;
use monero_wallet::address::{AddressType, MoneroAddress, Network};
use monero_wallet::ed25519::{Commitment, Point, Scalar};
use monero_wallet::extra::Extra;
use monero_wallet::primitives::keccak256;
use monero_wallet::ringct::EncryptedAmount;
use monero_wallet::transaction::Transaction;
use rand_core::{OsRng, RngCore};

pub use ducat_core::trust::{burn_message, BURN_DOMAIN, OUT_PROOF_HEADER};

/// Monero's `config::HASH_KEY_TXPROOF_V2`.
const TXPROOF_V2_SEP: &[u8] = b"TXPROOF_V2";
/// 32 bytes of Monero base58 is 44 characters; 64 bytes is 88.
const SECRET_CHARS: usize = 44;
const SIG_CHARS: usize = 88;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum ProofError {
    #[error("{0}")]
    Malformed(String),
    #[error("the proof does not verify")]
    BadProof,
    #[error("{0}")]
    Unsupported(String),
}

fn malformed(s: impl Into<String>) -> ProofError {
    ProofError::Malformed(s.into())
}

fn hash_to_scalar(data: &[u8]) -> DScalar {
    Scalar::hash(data).into()
}

/// Monero's varint: seven bits a byte, little-endian, high bit continues.
fn varint(mut n: u64) -> Vec<u8> {
    let mut v = Vec::with_capacity(10);
    loop {
        let b = (n & 0x7f) as u8;
        n >>= 7;
        if n == 0 {
            v.push(b);
            return v;
        }
        v.push(b | 0x80);
    }
}

fn point_bytes(p: &EdwardsPoint) -> [u8; 32] {
    p.compress().to_bytes()
}

fn decompress(b: &[u8; 32]) -> Result<EdwardsPoint, ProofError> {
    CompressedEdwardsY(*b).decompress().ok_or_else(|| malformed("not a point on the curve"))
}

/// The one address every DUCAT burn goes to (§9.5): both keys are Monero's
/// own `hash_to_ec` of a fixed string, so anyone can recompute them and see
/// that no discrete logarithm was ever chosen. The mainnet and stagenet
/// strings differ only by the network byte.
pub fn burn_address(network: Network) -> MoneroAddress {
    let key = |role: &[u8]| {
        let mut s = Vec::with_capacity(BURN_DOMAIN.len() + 1 + role.len());
        s.extend_from_slice(BURN_DOMAIN);
        s.push(0);
        s.extend_from_slice(role);
        Point::biased_hash(keccak256(s))
    };
    MoneroAddress::new(network, AddressType::Legacy, key(b"spend"), key(b"view"))
}

/// The burn address as a string, for the wallet screen and the spec.
#[uniffi::export]
pub fn monero_burn_address(stagenet: bool) -> String {
    burn_address(if stagenet { Network::Stagenet } else { Network::Mainnet }).to_string()
}

/// `wallet2::get_tx_proof`'s prefix hash: the transaction id and the message.
fn prefix_hash(txid: &[u8; 32], message: &[u8]) -> [u8; 32] {
    let mut data = Vec::with_capacity(32 + message.len());
    data.extend_from_slice(txid);
    data.extend_from_slice(message);
    keccak256(data)
}

/// `s_comm_2`, hashed to the challenge: msg ‖ D ‖ X ‖ Y ‖ sep ‖ R ‖ A ‖ B.
fn challenge(
    prefix: &[u8; 32],
    d: &EdwardsPoint,
    x: &EdwardsPoint,
    y: &EdwardsPoint,
    r_pub: &EdwardsPoint,
    a: &EdwardsPoint,
    b: Option<&EdwardsPoint>,
) -> DScalar {
    let mut buf = Vec::with_capacity(256);
    buf.extend_from_slice(prefix);
    buf.extend_from_slice(&point_bytes(d));
    buf.extend_from_slice(&point_bytes(x));
    buf.extend_from_slice(&point_bytes(y));
    buf.extend_from_slice(&keccak256(TXPROOF_V2_SEP));
    buf.extend_from_slice(&point_bytes(r_pub));
    buf.extend_from_slice(&point_bytes(a));
    buf.extend_from_slice(&b.map(point_bytes).unwrap_or([0u8; 32]));
    hash_to_scalar(&buf)
}

/// One `(D, sig)` pair of an out-proof, for one transaction key.
fn prove_key(prefix: &[u8; 32], r: &DScalar, a: &EdwardsPoint, b: Option<&EdwardsPoint>) -> ([u8; 32], [u8; 64]) {
    let r_pub = match b {
        Some(b) => b * r,
        None => G * r,
    };
    let d = a * r;
    let mut kb = [0u8; 64];
    OsRng.fill_bytes(&mut kb);
    let k = DScalar::from_bytes_mod_order_wide(&kb);
    let x = match b {
        Some(b) => b * k,
        None => G * k,
    };
    let y = a * k;
    let c = challenge(prefix, &d, &x, &y, &r_pub, a, b);
    let s = k - c * r;
    let mut sig = [0u8; 64];
    sig[..32].copy_from_slice(&c.to_bytes());
    sig[32..].copy_from_slice(&s.to_bytes());
    (point_bytes(&d), sig)
}

fn verify_key(
    prefix: &[u8; 32],
    r_pub: &EdwardsPoint,
    a: &EdwardsPoint,
    b: Option<&EdwardsPoint>,
    d: &EdwardsPoint,
    sig: &[u8; 64],
) -> bool {
    let c = match Option::<DScalar>::from(DScalar::from_canonical_bytes(sig[..32].try_into().unwrap())) {
        Some(c) => c,
        None => return false,
    };
    let s = match Option::<DScalar>::from(DScalar::from_canonical_bytes(sig[32..].try_into().unwrap())) {
        Some(s) => s,
        None => return false,
    };
    let x = match b {
        Some(b) => r_pub * c + b * s,
        None => r_pub * c + G * s,
    };
    let y = d * c + a * s;
    challenge(prefix, d, &x, &y, r_pub, a, b) == c
}

/// Make an `OutProofV2` for `txid` paid to `address`, from the transaction's
/// keys: the transaction key first, then any additional keys in output
/// order (a transaction paying a subaddress among several outputs carries
/// them). A burn pays a standard address alone and has one key.
pub fn make_out_proof_v2(
    txid: &[u8; 32],
    message: &[u8],
    keys: &[DScalar],
    address: &MoneroAddress,
) -> Result<String, ProofError> {
    if keys.is_empty() {
        return Err(malformed("a proof needs at least one transaction key"));
    }
    let prefix = prefix_hash(txid, message);
    let a: EdwardsPoint = address.view().into();
    let b: Option<EdwardsPoint> = address.is_subaddress().then(|| address.spend().into());
    let mut out = String::from(OUT_PROOF_HEADER);
    for r in keys {
        let (d, sig) = prove_key(&prefix, r, &a, b.as_ref());
        out.push_str(&monero_base58::encode(&d));
        out.push_str(&monero_base58::encode(&sig));
    }
    Ok(out)
}

/// The parts of a transaction a proof is checked against.
pub struct TxView {
    /// The transaction key from `extra`, and the additional keys if any.
    pub tx_key: [u8; 32],
    pub additional: Vec<[u8; 32]>,
    /// Each output's one-time key, and its plain amount for a v1 transaction.
    pub outputs: Vec<([u8; 32], Option<u64>)>,
    /// RingCT: one encrypted amount and one commitment per output.
    pub encrypted: Vec<EncryptedAmount>,
    pub commitments: Vec<[u8; 32]>,
}

impl TxView {
    pub fn from_tx(tx: &Transaction) -> Result<Self, ProofError> {
        let prefix = tx.prefix();
        let extra = Extra::read(&mut prefix.extra.as_slice()).map_err(|_| malformed("extra does not parse"))?;
        let (keys, additional) = extra.keys().ok_or_else(|| malformed("no transaction key in extra"))?;
        let tx_key = keys.first().ok_or_else(|| malformed("no transaction key in extra"))?.compress().to_bytes();
        let additional = additional.unwrap_or_default().into_iter().map(|p| p.compress().to_bytes()).collect();
        let outputs = prefix.outputs.iter().map(|o| (o.key.to_bytes(), o.amount)).collect();
        let (encrypted, commitments) = match tx {
            Transaction::V2 { proofs: Some(p), .. } => (
                p.base.encrypted_amounts.clone(),
                p.base.commitments.iter().map(|c| c.to_bytes()).collect(),
            ),
            _ => (Vec::new(), Vec::new()),
        };
        Ok(TxView { tx_key, additional, outputs, encrypted, commitments })
    }
}

fn base58_fixed(s: &str, want: usize) -> Result<Vec<u8>, ProofError> {
    let b = monero_base58::decode(s).ok_or_else(|| malformed("proof is not base58"))?;
    if b.len() != want {
        return Err(malformed("proof block has the wrong length"));
    }
    Ok(b)
}

/// Check an `OutProofV2` against the transaction it names and return the
/// amount it proves was paid to `address` — zero when the signatures hold
/// but no output belongs to the address. The signature is checked under
/// `message`, so a proof made for one persona's burn does not verify under
/// another's name.
pub fn check_out_proof_v2(
    view: &TxView,
    txid: &[u8; 32],
    address: &MoneroAddress,
    message: &[u8],
    proof: &str,
) -> Result<u64, ProofError> {
    let rest = proof.strip_prefix(OUT_PROOF_HEADER).ok_or_else(|| malformed("not an OutProofV2"))?;
    let pair = SECRET_CHARS + SIG_CHARS;
    if rest.is_empty() || rest.len() % pair != 0 {
        return Err(malformed("proof is not whole pairs"));
    }
    let n = rest.len() / pair;
    if n != 1 + view.additional.len() {
        return Err(malformed("the proof and the transaction disagree on how many keys there are"));
    }
    let prefix = prefix_hash(txid, message);
    let a: EdwardsPoint = address.view().into();
    let spend: EdwardsPoint = address.spend().into();
    let b = address.is_subaddress().then_some(&spend);
    let mut good = Vec::with_capacity(n);
    for i in 0..n {
        let block = &rest[i * pair..(i + 1) * pair];
        let d_bytes: [u8; 32] = base58_fixed(&block[..SECRET_CHARS], 32)?.try_into().unwrap();
        let sig: [u8; 64] = base58_fixed(&block[SECRET_CHARS..], 64)?.try_into().unwrap();
        let d = decompress(&d_bytes)?;
        let r_pub = decompress(if i == 0 { &view.tx_key } else { &view.additional[i - 1] })?;
        good.push(verify_key(&prefix, &r_pub, &a, b, &d, &sig).then_some(d));
    }
    if good.iter().all(|g| g.is_none()) {
        return Err(ProofError::BadProof);
    }
    // wallet2::check_tx_proof: derivation = 8·D, then every output that
    // derives to the address's spend key is counted.
    let mut received: u64 = 0;
    for (index, (key, plain)) in view.outputs.iter().enumerate() {
        let out_key = decompress(key)?;
        // The main key's secret covers every output; an additional key's
        // covers its own output only.
        let candidates: Vec<&EdwardsPoint> = good
            .iter()
            .enumerate()
            .filter(|(i, g)| g.is_some() && (*i == 0 || *i == index + 1))
            .map(|(_, g)| g.as_ref().unwrap())
            .collect();
        for d in candidates {
            let derivation = point_bytes(&d.mul_by_cofactor());
            let mut data = derivation.to_vec();
            data.extend(varint(index as u64));
            let scalar = hash_to_scalar(&data);
            if G * scalar + spend != out_key {
                continue;
            }
            let amount = match plain {
                Some(v) => *v,
                None => decrypt_amount(view, index, &scalar)?,
            };
            received = received.checked_add(amount).ok_or_else(|| malformed("amount overflow"))?;
            break;
        }
    }
    Ok(received)
}

/// RingCT's compact ECDH: the amount is eight bytes XOR the keccak of
/// "amount" ‖ scalar; the mask is H_s("commitment_mask" ‖ scalar). The
/// commitment is recomputed and must match, so a decrypted number is a
/// number the chain agrees with.
fn decrypt_amount(view: &TxView, index: usize, scalar: &DScalar) -> Result<u64, ProofError> {
    let enc = view.encrypted.get(index).ok_or_else(|| malformed("no encrypted amount for the output"))?;
    let commitment = view.commitments.get(index).ok_or_else(|| malformed("no commitment for the output"))?;
    let amount = match enc {
        EncryptedAmount::Compact { amount } => {
            let mut key = b"amount".to_vec();
            key.extend_from_slice(&scalar.to_bytes());
            let pad = keccak256(key);
            let mut a = [0u8; 8];
            for i in 0..8 {
                a[i] = amount[i] ^ pad[i];
            }
            u64::from_le_bytes(a)
        }
        EncryptedAmount::Original { .. } => {
            return Err(ProofError::Unsupported("pre-2020 encrypted amounts".into()));
        }
    };
    let mut mask_in = b"commitment_mask".to_vec();
    mask_in.extend_from_slice(&scalar.to_bytes());
    let mask = Scalar::hash(mask_in);
    let expect: EdwardsPoint = Commitment::new(mask, amount).commit().into();
    if point_bytes(&expect) != *commitment {
        return Err(malformed("the decrypted amount does not match the commitment"));
    }
    Ok(amount)
}

/// Make an out-proof from the bridge: keys as hex scalars (the send path
/// hands them out), the address as a string, the message as bytes.
#[uniffi::export]
pub fn monero_make_out_proof(
    txid_hex: String,
    message: Vec<u8>,
    tx_keys_hex: Vec<String>,
    address: String,
    stagenet: bool,
) -> Result<String, ProofError> {
    let txid = hex32(&txid_hex, "txid")?;
    let keys = tx_keys_hex
        .iter()
        .map(|h| {
            let b = hex32(h, "transaction key")?;
            Option::<DScalar>::from(DScalar::from_canonical_bytes(b)).ok_or_else(|| malformed("transaction key is not a scalar"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let addr = parse_address(&address, stagenet)?;
    make_out_proof_v2(&txid, &message, &keys, &addr)
}

/// Check an out-proof against a transaction the caller fetched (its hex
/// from the node), returning the amount paid to the address.
#[uniffi::export]
pub fn monero_check_out_proof(
    tx_hex: String,
    txid_hex: String,
    address: String,
    stagenet: bool,
    message: Vec<u8>,
    proof: String,
) -> Result<u64, ProofError> {
    let raw = crate::hex_to_bytes(&tx_hex).ok_or_else(|| malformed("transaction is not hex"))?;
    let tx = Transaction::read(&mut raw.as_slice()).map_err(|_| malformed("transaction does not parse"))?;
    let txid = hex32(&txid_hex, "txid")?;
    if tx.hash() != txid {
        return Err(malformed("the transaction is not the one the proof names"));
    }
    let addr = parse_address(&address, stagenet)?;
    check_out_proof_v2(&TxView::from_tx(&tx)?, &txid, &addr, &message, &proof)
}

fn parse_address(s: &str, stagenet: bool) -> Result<MoneroAddress, ProofError> {
    MoneroAddress::from_str(if stagenet { Network::Stagenet } else { Network::Mainnet }, s)
        .map_err(|_| malformed("not a Monero address for this network"))
}

fn hex32(h: &str, what: &str) -> Result<[u8; 32], ProofError> {
    crate::hex_to_bytes(h)
        .filter(|b| b.len() == 32)
        .map(|b| b.try_into().unwrap())
        .ok_or_else(|| malformed(format!("{what} is 32 bytes of hex")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hx(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    fn random_scalar() -> DScalar {
        let mut b = [0u8; 64];
        OsRng.fill_bytes(&mut b);
        DScalar::from_bytes_mod_order_wide(&b)
    }

    #[test]
    fn the_burn_address_is_the_same_every_time_and_a_standard_address() {
        let a = burn_address(Network::Stagenet);
        let b = burn_address(Network::Stagenet);
        assert_eq!(a.to_string(), b.to_string());
        assert!(!a.is_subaddress());
        assert_ne!(burn_address(Network::Mainnet).to_string(), a.to_string());
        assert_eq!(monero_burn_address(true), a.to_string());
        // Both keys are hash outputs, not products of the base point.
        assert_ne!(a.spend().compress().to_bytes(), a.view().compress().to_bytes());
    }

    #[test]
    fn base58_blocks_have_the_lengths_the_reference_wallet_slices_by() {
        assert_eq!(monero_base58::encode(&[7u8; 32]).len(), SECRET_CHARS);
        assert_eq!(monero_base58::encode(&[7u8; 64]).len(), SIG_CHARS);
    }

    /// A one-output RingCT transaction to `address`, paid by key `r`, with
    /// the amount encrypted the way the reference wallet does it.
    fn synthetic(r: &DScalar, address: &MoneroAddress, amount: u64) -> TxView {
        let a: EdwardsPoint = address.view().into();
        let spend: EdwardsPoint = address.spend().into();
        let d = a * r;
        let derivation = point_bytes(&d.mul_by_cofactor());
        let mut data = derivation.to_vec();
        data.extend(varint(0));
        let scalar = hash_to_scalar(&data);
        let key = G * scalar + spend;
        let mut key_in = b"amount".to_vec();
        key_in.extend_from_slice(&scalar.to_bytes());
        let pad = keccak256(key_in);
        let mut enc = amount.to_le_bytes();
        for i in 0..8 {
            enc[i] ^= pad[i];
        }
        let mut mask_in = b"commitment_mask".to_vec();
        mask_in.extend_from_slice(&scalar.to_bytes());
        let mask = Scalar::hash(mask_in);
        let commitment: EdwardsPoint = Commitment::new(mask, amount).commit().into();
        TxView {
            tx_key: point_bytes(&(G * r)),
            additional: vec![],
            outputs: vec![(point_bytes(&key), None)],
            encrypted: vec![EncryptedAmount::Compact { amount: enc }],
            commitments: vec![point_bytes(&commitment)],
        }
    }

    #[test]
    fn a_proof_verifies_for_its_transaction_message_and_address_only() {
        let r = random_scalar();
        let burn = burn_address(Network::Stagenet);
        let txid = [0x33u8; 32];
        let persona = [0x44u8; 32];
        let message = burn_message(&persona, "identity");
        let view = synthetic(&r, &burn, 12_345_000_000);
        let proof = make_out_proof_v2(&txid, &message, &[r], &burn).unwrap();
        assert!(proof.starts_with(OUT_PROOF_HEADER));
        assert_eq!(proof.len(), OUT_PROOF_HEADER.len() + SECRET_CHARS + SIG_CHARS);
        assert_eq!(check_out_proof_v2(&view, &txid, &burn, &message, &proof).unwrap(), 12_345_000_000);

        // Another persona's name in the message: the signature does not hold.
        let other = burn_message(&[0x45u8; 32], "identity");
        assert_eq!(check_out_proof_v2(&view, &txid, &burn, &other, &proof), Err(ProofError::BadProof));
        // Another transaction id: likewise.
        assert_eq!(check_out_proof_v2(&view, &[0x34u8; 32], &burn, &message, &proof), Err(ProofError::BadProof));
        // A different address: the proof's D is not r·A for it.
        let elsewhere = MoneroAddress::new(Network::Stagenet, AddressType::Legacy, Point::from(G * random_scalar()), Point::from(G * random_scalar()));
        assert_eq!(check_out_proof_v2(&view, &txid, &elsewhere, &message, &proof), Err(ProofError::BadProof));
        // A tampered amount on the chain does not decrypt to a matching commitment.
        let mut forged = synthetic(&r, &burn, 12_345_000_000);
        forged.commitments[0] = point_bytes(&(G * random_scalar()));
        assert!(matches!(check_out_proof_v2(&forged, &txid, &burn, &message, &proof), Err(ProofError::Malformed(_))));
        // A transaction that paid somebody else verifies the key and proves zero.
        let unrelated = synthetic(&r, &elsewhere, 5);
        assert_eq!(check_out_proof_v2(&unrelated, &txid, &burn, &message, &proof).unwrap(), 0);
    }

    #[test]
    fn a_proof_for_a_subaddress_uses_the_spend_key_as_the_base() {
        let r = random_scalar();
        let sub = MoneroAddress::new(Network::Stagenet, AddressType::Subaddress, Point::from(G * random_scalar()), Point::from(G * random_scalar()));
        let txid = [0x55u8; 32];
        let mut view = synthetic(&r, &sub, 777);
        // For a subaddress destination the transaction key in extra is r·B.
        let b: EdwardsPoint = sub.spend().into();
        view.tx_key = point_bytes(&(b * r));
        let proof = make_out_proof_v2(&txid, b"m", &[r], &sub).unwrap();
        assert_eq!(check_out_proof_v2(&view, &txid, &sub, b"m", &proof).unwrap(), 777);
    }

    #[test]
    fn the_bridge_functions_agree_with_the_library() {
        let r = random_scalar();
        let burn = monero_burn_address(true);
        let txid = [0x66u8; 32];
        let proof = monero_make_out_proof(hx(&txid), b"msg".to_vec(), vec![hx(&r.to_bytes())], burn.clone(), true).unwrap();
        let addr = parse_address(&burn, true).unwrap();
        let view = synthetic(&r, &addr, 9);
        assert_eq!(check_out_proof_v2(&view, &txid, &addr, b"msg", &proof).unwrap(), 9);
        assert!(monero_make_out_proof("zz".into(), vec![], vec![], burn, true).is_err());
    }
}
