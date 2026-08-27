#!/usr/bin/env python3
"""A second DUCAT implementation, written to disagree with the first (O21).

O21 says a vector set validated by one implementation encodes that
implementation's bugs as the specification. The only thing that closes it is a
second client that reads the same spec, reaches its own conclusions, and
disagrees somewhere.

This is that client, with one honest limitation stated up front: it was written
by the same author as the Rust. That is not a clean-room reimplementation and it
should not be described as one. What it can still catch is real, and is the
category of bug that actually costs interoperability:

  - places where the spec says something the Rust does not do,
  - places where the Rust does something the spec never says,
  - places where the spec admits two readings and the vectors silently pick one.

Every one of those is a bug a genuine second implementer would hit. None of them
are visible from inside the first implementation, because there the code *is* the
answer.

Written from §18 (and §4.3 for the backup format) rather than from the Rust
source. Where the spec was silent, the guess is marked GUESS with the reasoning,
because an undocumented guess is precisely what O21 is about.
"""

import json
import sys
import unicodedata
from pathlib import Path

# ---------------------------------------------------------------------------
# §18.5 Reject codes. Two implementations must fail the same way.
# ---------------------------------------------------------------------------

CODES = {
    "BadSig": 1, "Expired": 2, "Replay": 3, "CommitMismatch": 4,
    "PriceMismatch": 5, "UnsupportedVersion": 6, "UnsupportedSuite": 7,
    "UnsupportedProfile": 8, "UnknownField": 9, "Malformed": 10,
    "StateViolation": 11, "InsufficientCapacity": 12, "UntrustedArbiterSet": 13,
    "RateStale": 14, "Timeout": 15, "PolicyRefused": 16,
}


class Reject(Exception):
    def __init__(self, name, detail=""):
        super().__init__(f"{name}: {detail}")
        self.name = name
        self.code = CODES[name]
        self.detail = detail


# ---------------------------------------------------------------------------
# §18.1 Canonical CBOR
#
# Written as a decoder that refuses rather than one that repairs. The spec's
# framing is that determinism is load-bearing — every commitment in the protocol
# breaks if two clients encode one object differently — so anything that could be
# encoded two ways is a hard error here, not a normalisation.
# ---------------------------------------------------------------------------

MT_UINT, MT_NINT, MT_BYTES, MT_TEXT, MT_ARRAY, MT_MAP, MT_TAG, MT_SIMPLE = range(8)


# §18.1: normative, and the exact number is part of the wire format — two
# clients picking their own limits disagree about the same signed bytes.
MAX_DEPTH = 16


class Decoder:
    def __init__(self, buf):
        self.buf = buf
        self.i = 0
        self.depth = 0

    def need(self, n):
        if self.i + n > len(self.buf):
            raise Reject("Malformed", "truncated")
        out = self.buf[self.i:self.i + n]
        self.i += n
        return out

    def head(self):
        b = self.need(1)[0]
        mt, ai = b >> 5, b & 0x1F
        if ai < 24:
            return mt, ai, 0
        if ai == 24:
            v = self.need(1)[0]
            # §18.1 smallest form: 24 is the first value that legitimately needs
            # the one-byte head, so anything below it is an overlong encoding.
            if v < 24:
                raise Reject("Malformed", "overlong integer head")
            return mt, v, 1
        if ai == 25:
            v = int.from_bytes(self.need(2), "big")
            if v < 256:
                raise Reject("Malformed", "overlong integer head")
            return mt, v, 2
        if ai == 26:
            v = int.from_bytes(self.need(4), "big")
            if v < 65536:
                raise Reject("Malformed", "overlong integer head")
            return mt, v, 4
        if ai == 27:
            v = int.from_bytes(self.need(8), "big")
            if v < 2 ** 32:
                raise Reject("Malformed", "overlong integer head")
            return mt, v, 8
        if ai == 31:
            raise Reject("Malformed", "indefinite length")
        raise Reject("Malformed", f"reserved additional info {ai}")

    def value(self):
        if self.depth > MAX_DEPTH:
            raise Reject("Malformed", "nesting deeper than 16 levels")
        mt, arg, _ = self.head()
        if mt == MT_UINT:
            return ("uint", arg)
        if mt == MT_NINT:
            # Nothing in the protocol carries a negative number: money is
            # unsigned piconero (§18.2) and map keys are unsigned (§18.1).
            # GUESS: the spec never names negative integers as illegal. Refusing
            # them keeps the value space to what the protocol actually uses.
            raise Reject("Malformed", "negative integers are not used")
        if mt == MT_BYTES:
            return ("bytes", self.need(arg))
        if mt == MT_TEXT:
            raw = self.need(arg)
            try:
                s = raw.decode("utf-8")
            except UnicodeDecodeError:
                raise Reject("Malformed", "text is not UTF-8")
            if unicodedata.normalize("NFC", s) != s:
                raise Reject("Malformed", "text is not NFC-normalized")
            return ("text", s)
        if mt == MT_ARRAY:
            self.depth += 1
            out = ("array", [self.value() for _ in range(arg)])
            self.depth -= 1
            return out
        if mt == MT_MAP:
            self.depth += 1
            items = []
            prev_key_bytes = None
            for _ in range(arg):
                key_start = self.i
                k = self.value()
                key_bytes = self.buf[key_start:self.i]
                if k[0] != "uint":
                    raise Reject("Malformed", "map keys must be unsigned integers")
                if prev_key_bytes is not None:
                    if key_bytes == prev_key_bytes:
                        raise Reject("Malformed", "duplicate map key")
                    if key_bytes < prev_key_bytes:
                        raise Reject("Malformed", "map keys must ascend bytewise")
                prev_key_bytes = key_bytes
                items.append((k[1], self.value()))
            self.depth -= 1
            return ("map", items)
        if mt == MT_TAG:
            raise Reject("Malformed", "tags are not allowed (empty allowlist)")
        # Major type 7: floats and simple values.
        raise Reject("Malformed", "floats and simple values are not allowed")


def decode_canonical(buf):
    d = Decoder(buf)
    v = d.value()
    if d.i != len(buf):
        raise Reject("Malformed", "trailing bytes")
    return v


def encode(v):
    kind, val = v

    def head(mt, arg):
        if arg < 24:
            return bytes([mt << 5 | arg])
        if arg < 256:
            return bytes([mt << 5 | 24, arg])
        if arg < 65536:
            return bytes([mt << 5 | 25]) + arg.to_bytes(2, "big")
        if arg < 2 ** 32:
            return bytes([mt << 5 | 26]) + arg.to_bytes(4, "big")
        return bytes([mt << 5 | 27]) + arg.to_bytes(8, "big")

    if kind == "uint":
        return head(MT_UINT, val)
    if kind == "bytes":
        return head(MT_BYTES, len(val)) + val
    if kind == "text":
        b = val.encode("utf-8")
        return head(MT_TEXT, len(b)) + b
    if kind == "array":
        return head(MT_ARRAY, len(val)) + b"".join(encode(x) for x in val)
    if kind == "map":
        ordered = sorted(val, key=lambda kv: encode(("uint", kv[0])))
        return head(MT_MAP, len(ordered)) + b"".join(
            encode(("uint", k)) + encode(x) for k, x in ordered
        )
    raise Reject("Malformed", f"cannot encode {kind}")


# ---------------------------------------------------------------------------
# §18.3 Domain separation
# ---------------------------------------------------------------------------

PREFIX = b"DUCAT-v1"


def sig_input(object_type, suite_id, canonical_bytes):
    """"DUCAT-v1" || 0x00 || object_type || 0x00 || suite_id || 0x00 || bytes"""
    return (PREFIX + b"\x00" + object_type.encode() + b"\x00"
            + bytes([suite_id]) + b"\x00" + canonical_bytes)


def commit(purpose, canonical_bytes):
    """SHA-256("DUCAT-v1" || 0x00 || purpose || 0x00 || canonical_bytes)"""
    import hashlib
    return hashlib.sha256(
        PREFIX + b"\x00" + purpose.encode() + b"\x00" + canonical_bytes
    ).digest()


# §18.3's prose names the purpose labels "offer_commit", "receipt", "chain",
# "market_genesis", while the transcript vector describes them in passing as
# Offer / ChainLink. Those cannot both be the bytes that go into the hash, so
# both spellings were tried and reported rather than one being silently retried
# until something matched. The spec spelling is the correct one — confirmed
# against `commitments_are_domain_separated_by_purpose`, which publishes all four
# digests. The vector's prose was using the first implementation's enum names.
PURPOSE_SPEC = {"offer": "offer_commit", "receipt": "receipt",
                "chain": "chain", "market": "market_genesis"}
PURPOSE_ALT = {"offer": "Offer", "receipt": "Receipt",
               "chain": "ChainLink", "market": "MarketGenesis"}


# ---------------------------------------------------------------------------
# Suites. 1 = Ed25519, 2 = P-256 (§4.1's hardware-forced fallback).
# ---------------------------------------------------------------------------

P256_N = 0xFFFFFFFF00000000FFFFFFFFFFFFFFFFBCE6FAADA7179E84F3B9CAC2FC632551


def verify_sig(suite, pubkey, sig, message):
    if suite == 1:
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
        from cryptography.exceptions import InvalidSignature
        if len(pubkey) != 32:
            raise Reject("Malformed", "ed25519 public keys are 32 bytes")
        try:
            Ed25519PublicKey.from_public_bytes(pubkey).verify(sig, message)
        except InvalidSignature:
            raise Reject("BadSig", "signature does not verify")
        return

    if suite == 2:
        from cryptography.hazmat.primitives.asymmetric import ec, utils
        from cryptography.hazmat.primitives import hashes
        from cryptography.exceptions import InvalidSignature

        # §18.3(2): exactly one public-key encoding is legal — compressed, 33
        # bytes, tag checked explicitly. The spec is emphatic that this is not
        # about SEC1 legality but about uniqueness: a second encoding of one key
        # is a second canonical object and a second transcript hash.
        if len(pubkey) != 33:
            raise Reject("Malformed", "public key must be 33 bytes")
        if pubkey[0] not in (0x02, 0x03):
            raise Reject("Malformed", f"illegal point tag 0x{pubkey[0]:02x}")
        try:
            pk = ec.EllipticCurvePublicKey.from_encoded_point(ec.SECP256R1(), pubkey)
        except ValueError:
            raise Reject("Malformed", "point is not on the curve")

        if len(sig) != 64:
            raise Reject("Malformed", "signature must be 64 bytes of r||s")
        r = int.from_bytes(sig[:32], "big")
        s = int.from_bytes(sig[32:], "big")
        # §18.3(1): low-s only, and the high-s twin is REJECTED rather than
        # normalized — normalizing would make two byte strings each "the"
        # signature, so the transcript hash would depend on which arrived.
        if s > P256_N // 2:
            raise Reject("BadSig", "high-s signature; low-s form is required")
        if not (0 < r < P256_N) or not (0 < s < P256_N):
            raise Reject("BadSig", "r or s out of range")
        try:
            pk.verify(utils.encode_dss_signature(r, s), message,
                      ec.ECDSA(hashes.SHA256()))
        except InvalidSignature:
            raise Reject("BadSig", "signature does not verify")
        return

    raise Reject("UnsupportedSuite", f"suite {suite}")


# ---------------------------------------------------------------------------
# §18.6 Negotiation
# ---------------------------------------------------------------------------

def negotiate(offered_versions, offered_suites, my_versions, my_suite_preference):
    common_v = set(offered_versions) & set(my_versions)
    if not common_v:
        raise Reject("UnsupportedVersion", "no mutually supported version")
    version = max(common_v)  # ordered by construction; higher means newer

    # Suites are NOT compared numerically. The payer walks its own preference
    # list and takes the first suite the offer also carries.
    offered = set(offered_suites)
    for s in my_suite_preference:
        if s in offered:
            return version, s
    raise Reject("UnsupportedSuite", "no permitted suite in common")


# ---------------------------------------------------------------------------
# §18.4 State machine
# ---------------------------------------------------------------------------

TERMINAL = {"Aborted", "Cancelled", "Disputed", "Settled", "Claimed"}


def deadline(state, mode):
    """§6.2's table. Returns seconds, or None where the state is unbounded.

    The two mode-dependent rows are the ones §18.4.1 had to spell out because the
    table hides them: ACCEPTED is 60 s for direct and fast but 300 s for escrow,
    whose window is spent on multi-round multisig setup; and FUNDED's 30 s is a
    fast/1 wait for TXPROOF that does not exist in the other modes.
    """
    if state == "Offered":
        return 10
    if state == "Quoted":
        return 30           # TapPresent.expiry, capped at 30 s
    if state == "Accepted":
        return 300 if mode == "Escrow" else 60
    if state == "Funded":
        return 30 if mode == "Fast" else None
    if state == "Delivered":
        return 120
    if state == "Closed":
        # The contact window (§4). It changes no state, which is why §18.4's
        # table omitted it until 0.45 — and why omitting it is still wrong: a
        # client without it keeps session keys alive forever.
        return 120
    # METERING is deliberately unbounded (§18.4.1(8)): its limit is
    # terms.meter_max_s, which the machine does not hold, so expiry arrives as an
    # explicit MeterExpired event instead.
    return None


def on_expiry(state, mode):
    if state == "Closed":
        return "Closed", "None"   # window closes, session keys destroyed
    if state == "Offered":
        return "Idle", "DiscardSilently"
    if state == "Quoted":
        return "Aborted", "None"
    if state == "Accepted":
        return "Aborted", "RecoverEscrowFunds" if mode == "Escrow" else "None"
    if state == "Funded":
        return "Aborted", "None"
    if state == "Delivered":
        return "Closed", "EmitPaymentEvidence"
    raise Reject("StateViolation", f"{state} has no expiry action")


def transition(state, event, origin, mode, role, elapsed=0):
    """Returns (next_state, effect). Raises Reject(StateViolation) otherwise.

    §18.4.1(1) is the rule most likely to be got wrong, and the spec says so:
    direction constrains the *originator*, never the evaluator. Both parties run
    this same function over the same message and must agree, so `origin` travels
    with the event and `role` is only ever used for deciding what to do next —
    never for deciding whether the message was legal.
    """
    if state in TERMINAL:
        raise Reject("StateViolation", f"{state} is absorbing")

    e = event

    # §18.4.1(6): elapsed time in an unbounded state is a no-op, not an error.
    # Clients poll on their own schedule and must not reach different states by
    # polling more often.
    if e == "Elapsed":
        d = deadline(state, mode)
        if d is None or elapsed < d:
            return state, "None"
        return on_expiry(state, mode)   # boundary is inclusive

    if state == "Idle" and e == "TapPresent":
        return "Offered", "None"
    if state == "Offered":
        if e == "FullOffer":
            return "Quoted", "None"
        if e == "Timeout":
            # §18.4: back to Idle silently — no screen was ever shown.
            return "Idle", "DiscardSilently"
    if state == "Quoted":
        if e == "Accept":
            if origin != "Payer":
                raise Reject("StateViolation", "only the payer may emit ACCEPT")
            return "Accepted", "None"
        if e == "MeterStart":
            return "Metering", "None"
        if e in ("Abort", "Timeout"):
            return "Aborted", "None"
    if state == "Accepted":
        if e == "Fund":
            return "Funded", "None"
        if e == "Cancel":
            return "Cancelled", "None"
        if e == "Timeout":
            # §18.4.1(3): escrow expiry must run fund recovery, not a bare abort.
            if mode == "Escrow":
                return "Aborted", "RecoverEscrowFunds"
            return "Aborted", "None"
    if state == "Metering":
        if e == "MeterStop":
            return "Accepted", "None"
        if e == "MeterExpired":
            return "Closed", "EmitDebtEvidence"
        if e == "Abort":
            # §18.4.1(7): only the operator may void a live meter cleanly.
            if origin != "Payee":
                raise Reject("StateViolation",
                             "a payer leaving a live meter is abandonment, not an abort")
            return "Aborted", "None"
    if state == "Funded":
        if e == "TxId" and mode == "Fast":
            return "Provisional", "None"
        if e == "Proof":
            return "Delivered", "None"
        if e == "Dispute" and mode == "Escrow":
            return "Disputed", "None"
        if e == "Timeout" and mode == "Fast":
            return "Aborted", "None"
    if state == "Provisional":
        if e == "Proof":
            return "Delivered", "None"
        if e == "Dispute" and mode == "Escrow":
            return "Disputed", "None"
    if state == "Delivered":
        if e == "Receipt":
            return "Closed", "None"
        if e == "Timeout":
            return "Closed", "EmitPaymentEvidence"
        if e == "Dispute" and mode == "Escrow":
            return "Disputed", "None"
    if state == "Closed":
        if e == "ConfirmationsReached" and mode == "Fast":
            return "Settled", "ReleaseCapacity"
        if e == "CureWindowExpired" and mode == "Fast":
            return "Claimed", "FileSlashClaim"
        if e in ("ContactOffer", "ContactAccept"):
            return "Closed", "None"

    # §18.4: anything not listed is STATE_VIOLATION, never a silent ignore.
    raise Reject("StateViolation", f"{e} is not legal in {state}")


# ---------------------------------------------------------------------------
# §4.3 Backup
# ---------------------------------------------------------------------------

BACKUP_MAGIC = b"DUCAT-BACKUP-v1"


def backup_import(blob, passphrase, kdf):
    head = len(BACKUP_MAGIC) + 16 + 24
    if len(blob) < head + 16:
        raise Reject("Malformed", "truncated backup")
    if blob[:len(BACKUP_MAGIC)] != BACKUP_MAGIC:
        raise Reject("Malformed", "not a DUCAT backup")
    salt = blob[len(BACKUP_MAGIC):len(BACKUP_MAGIC) + 16]
    nonce = blob[len(BACKUP_MAGIC) + 16:head]

    from argon2.low_level import hash_secret_raw, Type
    key = hash_secret_raw(
        secret=passphrase, salt=salt,
        time_cost=kdf["iterations"], memory_cost=kdf["memory_kib"],
        parallelism=kdf["lanes"], hash_len=kdf["output_len"], type=Type.ID,
    )
    from Crypto.Cipher import ChaCha20_Poly1305
    c = ChaCha20_Poly1305.new(key=key, nonce=nonce)
    c.update(BACKUP_MAGIC)
    try:
        plain = c.decrypt_and_verify(blob[head:-16], blob[-16:])
    except ValueError:
        raise Reject("BadSig", "wrong passphrase, or the backup has been altered")

    fields = dict(decode_canonical(plain)[1])
    if fields[0][1] != 1:
        raise Reject("UnsupportedVersion", "unknown backup version")
    # §4.3.2: an import is a trust boundary. Decryption proves the file was not
    # tampered with; it does not prove the contents were ever sane.
    device_unlock_at = fields[9][1]
    app_secret_at = fields[10][1]
    if app_secret_at < device_unlock_at:
        raise Reject("PolicyRefused",
                     "verification thresholds must ascend")
    if fields[11][1] == 0:
        raise Reject("PolicyRefused", "zero-validity secret can never be satisfied")
    for share in fields[14][1]:
        sm = dict(share[1])
        if len(sm[1][1]) == 0:
            raise Reject("Malformed", "escrow share with no key file restores nothing")
    return fields


# ---------------------------------------------------------------------------
# Runner
# ---------------------------------------------------------------------------

class Results:
    def __init__(self):
        self.passed = 0
        self.disagreements = []

    def ok(self):
        self.passed += 1

    def bad(self, category, name, why, detail):
        self.disagreements.append((category, name, why, detail))


def expect_reject(r, cat, case, fn):
    """Run fn; compare against the case's expectation."""
    want_ok = case["expect"].get("ok", True)
    try:
        got = fn()
    except Reject as ex:
        if want_ok:
            r.bad(cat, case["name"], case.get("why", ""),
                  f"we rejected ({ex.name}) where the vector expects success")
        elif ex.code != case["expect"].get("reject_code", ex.code):
            r.bad(cat, case["name"], case.get("why", ""),
                  f"we said {ex.name}({ex.code}), vector says "
                  f"{case['expect'].get('reject_name')}({case['expect'].get('reject_code')})")
        else:
            r.ok()
        return None
    except Exception as ex:  # a crash is a disagreement too
        r.bad(cat, case["name"], case.get("why", ""), f"crashed: {type(ex).__name__}: {ex}")
        return None
    if not want_ok:
        r.bad(cat, case["name"], case.get("why", ""),
              f"we accepted where the vector expects "
              f"{case['expect'].get('reject_name')}")
        return None
    r.ok()
    return got


def unhex(s):
    return bytes.fromhex(s)


def run_codec(cases, r):
    for c in cases:
        def go(c=c):
            return encode(decode_canonical(unhex(c["input_hex"])))
        out = expect_reject(r, "codec", c, go)
        if out is not None and out.hex() != c["expect"]["reencodes_to_hex"]:
            r.passed -= 1
            r.bad("codec", c["name"], c.get("why", ""),
                  f"re-encoded to {out.hex()}, vector says "
                  f"{c['expect']['reencodes_to_hex']}")


def run_signing_verify(cases, r):
    for c in cases:
        def go(c=c):
            msg = sig_input(c["verify_as"], c["suite"], unhex(c["object_hex"]))
            verify_sig(c["suite"], unhex(c["pubkey_hex"]), unhex(c["sig_hex"]), msg)
            return True
        expect_reject(r, "signing", c, go)


def run_signing_pubkey(cases, r):
    """Key parsing alone. The signature is deliberately junk: these cases must
    fail on the encoding of the key, before any verification is attempted."""
    for c in cases:
        def go(c=c):
            verify_sig(c["suite"], unhex(c["pubkey_hex"]), b"\x00" * 64, b"")
            return True
        expect_reject(r, "signing", c, go)


def run_negotiate(cases, r):
    for c in cases:
        def go(c=c):
            v, sui = negotiate(c["offered"]["versions"], c["offered"]["suites"],
                               c["local_versions"], c["payer_preference"])
            return {"version": v, "suite": sui}
        got = expect_reject(r, "negotiate", c, go)
        if got is None:
            continue
        for k in ("version", "suite"):
            if got[k] != c["expect"][k]:
                r.passed -= 1
                r.bad("negotiate", c["name"], c.get("why", ""),
                      f"{k}: we chose {got[k]}, vector says {c['expect'][k]}")
                break


def run_commit_purposes(cases, r):
    for c in cases:
        body = unhex(c["input_hex"])
        wrong = {p: (commit(p, body).hex(), want)
                 for p, want in c["expect"]["digests_by_purpose"].items()
                 if commit(p, body).hex() != want}
        if wrong:
            r.bad("commit", c["name"], c.get("why", ""),
                  f"purpose digests disagree: {wrong}")
        else:
            r.ok()


def run_commit_substitution(cases, r):
    for c in cases:
        want = unhex(c["offer_commit_hex"])
        genuine = commit("offer_commit", unhex(c["genuine_offer_hex"])) == want
        stripped = commit("offer_commit", unhex(c["stripped_offer_hex"])) == want
        if genuine == c["expect"]["genuine"]["ok"] and stripped == c["expect"]["stripped"]["ok"]:
            r.ok()
        else:
            r.bad("commit", c["name"], c.get("why", ""),
                  f"genuine matches={genuine}, stripped matches={stripped}")


def run_transcript_substitution(cases, r):
    for c in cases:
        matches = commit("offer_commit", unhex(c["delivered_offer_hex"])) \
            == unhex(c["tap_offer_commit_hex"])
        # The delivered offer must NOT match the tap's commitment.
        if matches == c["expect"]["ok"]:
            r.ok()
        else:
            r.bad("transcript", c["name"], c.get("why", ""),
                  f"delivered offer matches tap commitment: {matches}")


def run_transcript_replay(cases, r):
    """Recompute the published commitments. That is where two implementations
    actually diverge, and it is the part the vector pins."""
    for c in cases:
        exp = c["expect"]
        want = exp.get("accept_chain_link_hex")
        if want is None:
            r.ok()
            continue
        got = commit(PURPOSE_SPEC["chain"], unhex(c["accept_hex"])).hex()
        if got == want:
            r.ok()
        else:
            alt = commit(PURPOSE_ALT["chain"], unhex(c["accept_hex"])).hex()
            r.bad("transcript", c["name"], c.get("why", ""),
                  f"chain link: vector says {want}; §18.3 labels give {got}, "
                  f"the prose's enum names give {alt}")


def check_step(r, c, state, mode, role, ev, expect):
    """Returns the next state, or None if the case failed."""
    name = ev["name"]
    origin = ev.get("from")
    elapsed = ev.get("elapsed_s", 0)
    want_ok = expect.get("ok", True)
    try:
        nxt, eff = transition(state, name, origin, mode, role, elapsed)
    except Reject as ex:
        if want_ok:
            r.bad("state", c["name"], c.get("why", ""),
                  f"{name} from {state}: we rejected ({ex.detail}), "
                  f"vector expects {expect.get('next')}")
            return None
        if ex.code != expect["reject_code"]:
            r.bad("state", c["name"], c.get("why", ""),
                  f"{name} from {state}: we said {ex.name}({ex.code}), "
                  f"vector says {expect['reject_name']}({expect['reject_code']})")
            return None
        return "__rejected__"
    if not want_ok:
        r.bad("state", c["name"], c.get("why", ""),
              f"{name} from {state}: we reached {nxt}, vector expects "
              f"{expect['reject_name']}")
        return None
    if nxt != expect["next"]:
        r.bad("state", c["name"], c.get("why", ""),
              f"{name} from {state}: we reached {nxt}, vector says {expect['next']}")
        return None
    if eff != expect["effect"]:
        r.bad("state", c["name"], c.get("why", ""),
              f"{name} from {state}: effect {eff}, vector says {expect['effect']}")
        return None
    return nxt


def run_state(cases, r):
    for c in cases:
        state, mode, role = c["from"], c["mode"], c["role"]
        ok = True
        if "deadline_s" in c:
            got = deadline(state, mode)
            if got != c["deadline_s"]:
                r.bad("state", c["name"], c.get("why", ""),
                      f"deadline for {state}/{mode}: we say {got}, "
                      f"vector says {c['deadline_s']}")
                ok = False
        for step in c["steps"]:
            nxt = check_step(r, c, state, mode, role, step["event"], step["expect"])
            if nxt is None:
                ok = False
                break
            if nxt == "__rejected__":
                break
            state = nxt
        if ok:
            r.ok()


def run_backup(cases, r):
    for c in cases:
        kdf = c.get("kdf") or {"memory_kib": 65536, "iterations": 3,
                               "lanes": 1, "output_len": 32}

        def go(c=c, kdf=kdf):
            return backup_import(unhex(c["blob_hex"]),
                                 c["passphrase_utf8"].encode(), kdf)
        got = expect_reject(r, "backup", c, go)
        if got is None or "decoded" not in c["expect"]:
            continue
        d = c["expect"]["decoded"]
        for key, want, label in [
            (1, d["persona_suite"], "persona_suite"),
            (3, d["monero_seed"], "monero_seed"),
            (4, d["monero_restore_height"], "monero_restore_height"),
            (7, d["created"], "created"),
        ]:
            if got[key][1] != want:
                r.passed -= 1
                r.bad("backup", c["name"], c.get("why", ""),
                      f"{label}: we read {got[key][1]!r}, vector says {want!r}")
                break


# `kind` is the single discriminator. Nothing is routed by filename or by
# guessing at which fields a case happens to carry — which is what a second
# implementer had to do before 0.46 (§18.11).
def run_object(cases, r):
    """Decode one wire object, re-encode, compare. The narrowest agreement two
    implementations need — everything downstream hashes canonical objects.

    This decoder is generic: it checks canonical form and the declared type
    against the registry, without modelling each object's fields. That is
    enough to catch the encoding divergences that matter and honest about what
    it does not check."""
    for c in cases:
        def go(c=c):
            v = decode_canonical(unhex(c["object_hex"]))
            body = dict(v[1])
            declared = body.get(0, (None, None))[1]
            want = OBJECT_TYPE_CODES.get(c["object_type"])
            if want is None:
                raise Reject("Malformed", f"unregistered type {c['object_type']}")
            if declared != want:
                raise Reject("Malformed",
                             f"object declares type {declared}, expected {want}")
            return encode(v)
        out = expect_reject(r, "object", c, go)
        if out is not None and out.hex() != c["expect"]["reencodes_to_hex"]:
            r.passed -= 1
            r.bad("object", c["name"], c.get("why", ""),
                  f"re-encoded to {out.hex()}, vector says "
                  f"{c['expect']['reencodes_to_hex']}")


# §18.4.2's type registry. Codes 13-22 were added at 0.47, replacing improvised
# `CANCEL + 100` values that decoders never checked.
OBJECT_TYPE_CODES = {
    "TapPresent": 1, "FullOffer": 2, "ACCEPT": 3, "RECEIPT": 4, "TXPROOF": 5,
    "REFUND": 6, "CANCEL": 7, "MANDATE": 8, "CONTACT_OFFER": 9,
    "CONTACT_ACCEPT": 10, "bond_proof": 11, "attestation": 12,
    "DISPUTE": 13, "RULING": 14, "HAIL": 15, "HAIL_REPLY": 16, "TapStatic": 17,
    "TXID": 18, "ESCROW_SETUP": 19, "ESCROW_READY": 20, "RELEASE": 21,
    "SLASH_CLAIM": 22, "MESSAGE": 23, "PREKEY_BUNDLE": 24,
    "SEALED_MESSAGE": 25, "LOG_HEAD": 26, "BOARD_NOTICE": 27,
}


# --- §8.2 / §17.4 / §17.5 contract logic ----------------------------------
#
# Reimplemented from the spec, not from core/. These are the decisions money
# depends on, and two clients that encode identically can still decide
# differently — which is the whole reason O21 exists.

BUYER, SELLER, ARBITER = 0, 1, 2


def run_escrow_ceremony(cases, r):
    for c in cases:
        eid = c["escrow_id_hex"]
        rounds_required = c["rounds_required"]
        rnd, seen, done = 0, [False] * 3, False
        ok = True
        for step in c["steps"]:
            want_ok = step["expect"].get("ok", True)
            err = None
            if done:
                err = ("StateViolation", "ceremony finished")
            elif step["round"] != rnd:
                err = ("StateViolation", "out-of-order round")
            elif seen[step["from_index"]]:
                err = ("Replay", "duplicate contribution")
            if err is None:
                seen[step["from_index"]] = True
                if all(seen):
                    rnd += 1
                    seen = [False] * 3
                    if rnd >= rounds_required:
                        done = True
            if want_ok != (err is None):
                r.bad("contract", c["name"], c.get("why", ""),
                      f"round {step['round']} from {step['from_index']}: "
                      f"we {'accepted' if err is None else 'refused'}, vector says otherwise")
                ok = False
                break
            if err and CODES[err[0]] != step["expect"]["reject_code"]:
                r.bad("contract", c["name"], c.get("why", ""),
                      f"we said {err[0]}, vector says {step['expect']['reject_name']}")
                ok = False
                break
        if ok:
            r.ok()


def run_escrow_ready(cases, r):
    for c in cases:
        def go(c=c):
            reports = c["reports"]
            if len(reports) != 3:
                raise Reject("PolicyRefused", "every participant must report")
            seen = set()
            for rep in reports:
                if rep["from_index"] in seen:
                    raise Reject("Replay", "two reports from one participant")
                seen.add(rep["from_index"])
                if rep["threshold"] != 2 or rep["total"] != 3:
                    raise Reject("PolicyRefused", "escrow must be 2-of-3")
                if rep["ms_address"] != reports[0]["ms_address"]:
                    raise Reject("CommitMismatch", "different wallets formed")
                if rep["arbiter"] != reports[0]["arbiter"]:
                    raise Reject("UntrustedArbiterSet", "arbiter disagreement")
            if reports[0]["arbiter"] not in c["trusted_arbiters"]:
                raise Reject("UntrustedArbiterSet", "arbiter not in the market's set")
            return reports[0]["ms_address"]
        got = expect_reject(r, "contract", c, go)
        want = c["expect"].get("agreed_address")
        if got is not None and want is not None and got != want:
            r.passed -= 1
            r.bad("contract", c["name"], c.get("why", ""),
                  f"agreed on {got}, vector says {want}")


def run_escrow_release(cases, r):
    for c in cases:
        def go(c=c):
            if c["amount_pxmr"] == 0:
                raise Reject("PriceMismatch", "a release of zero closes nothing")
            if c["amount_pxmr"] > c["escrowed_pxmr"]:
                raise Reject("PriceMismatch", "release exceeds what is held")
            if c["to"] not in c["allowed_destinations"]:
                raise Reject("PolicyRefused", "destination is not a party to this escrow")
            return True
        expect_reject(r, "contract", c, go)


# §17.8's ladder. Reproduced here rather than imported, so a divergence shows up.
CAPACITY_BUCKETS = [0, 1_000_000_000, 2_000_000_000, 5_000_000_000,
                    10_000_000_000, 20_000_000_000, 50_000_000_000,
                    100_000_000_000, 200_000_000_000, 500_000_000_000,
                    1_000_000_000_000, 2_000_000_000_000, 5_000_000_000_000,
                    10_000_000_000_000, 20_000_000_000_000, 50_000_000_000_000,
                    100_000_000_000_000]


def run_bond_check(cases, r):
    for c in cases:
        def go(c=c):
            if c["issued"] > c["now"] + 120:
                raise Reject("Expired", "dated in the future")
            if c["now"] - c["issued"] > c["max_age_s"]:
                raise Reject("Expired", "stale")
            if c["arbiter_set_id_hex"] not in c["trusted_arbiter_sets"]:
                raise Reject("UntrustedArbiterSet", "unknown arbiter set")
            if c["capacity_bucket"] not in CAPACITY_BUCKETS:
                raise Reject("Malformed", "not a ladder value")
            if c["capacity_bucket"] < c["fare_pxmr"]:
                raise Reject("InsufficientCapacity", "bucket does not cover the fare")
            if c["capacity_bucket"] > c["bond_amount_pxmr"]:
                raise Reject("InsufficientCapacity", "capacity above the bond")
            if c["bond_amount_pxmr"] < c["fare_pxmr"]:
                raise Reject("InsufficientCapacity", "bond smaller than the fare")
            return True
        expect_reject(r, "contract", c, go)


def run_slash_check(cases, r):
    for c in cases:
        def go(c=c):
            if c["claim_pxmr"] > c["agreed_pxmr"]:
                raise Reject("PriceMismatch", "claim exceeds what was agreed")
            # Two reasons, and nothing else. This read `else:` — so any
            # unrecognised reason was handled as a double spend, which is the
            # one that skips the waiting period. An implementation that rounds
            # an unknown cause to the nearest one it knows is honouring a claim
            # on somebody's bond for a cause nobody defined.
            if c["reason"] not in (1, 2):
                raise Reject("Malformed", "unknown slash reason")
            if c["reason"] == 1:
                if c["elapsed_blocks"] < c["cure_blocks"]:
                    raise Reject("PolicyRefused", "cure window has not expired")
                if "key_image_hex" in c:
                    raise Reject("Malformed", "a cure-window claim carries no key image")
            else:
                if "key_image_hex" not in c:
                    raise Reject("Malformed", "a double-spend claim must carry the key image")
            return True
        expect_reject(r, "contract", c, go)


# --- §16.9 / §16.10 contacts and messages -----------------------------------
#
# Written from the spec text, not from core/contact.rs. These are the first
# vectors that put *text* on the wire at the field level: §7.5's memos shipped
# with no coverage here at all, because run_object is generic over fields, so
# no two decoders had ever been asked to agree on a text bound.

MAX_DISPLAY_NAME_CHARS = 32
MAX_MESSAGE_CHARS = 2000

MAX_RECORD_KEY_CHARS = 128

# §18.4.2 field keys. 147-156 held the route-blob card and are burned rather
# than reused: an old card decoding as a new one under different meanings is the
# divergence the registry exists to prevent.
MSG_SEQ, MSG_PREV, MSG_BODY, MSG_TS = 157, 158, 159, 160
MSG_KIND, MSG_AMOUNT, MSG_TXID, MSG_PAYTO = 178, 179, 180, 181
MSG_ITEMS, MSG_TAX = 183, 184
MSG_RE_SEQ, MSG_RE_OWN = 192, 193
MSG_ETA = 213
MSG_PAYLOAD, MSG_ROUND, MSG_CEREMONY = 214, 215, 216
MSG_ATT_RECORD, MSG_ATT_KEY, MSG_ATT_NONCE = 194, 195, 196
MSG_ATT_LEN, MSG_ATT_HASH, MSG_ATT_MIME, MSG_ATT_NAME = 197, 198, 199, 200
# §15.12 — the live-position reference (kind 11): record + stream key.
MSG_POS_RECORD, MSG_POS_STREAM = 218, 219
HEAD_BUNDLE = 177
HEAD_READ, HEAD_RING = 201, 202
# HAIL_NOTICE (§16.17) — the one object on a public surface.
HN_VERSION, HN_CARD, HN_DEST, HN_FARE, HN_EXPIRY = 203, 204, 205, 206, 207
HN_ORIGIN_CELL, HN_DEST_CELL = 208, 209
# §16.18 RENTAL_NOTICE — the listing. A second public-board object with a
# tighter rule than the hail's: it outlives the day it was posted, so it may
# not pin the thing any finer than ~5 km.
RN_VERSION = 220
RN_CARD = 221
RN_KIND = 222
RN_TITLE = 223
RN_AREA = 224
RN_CELL = 225
RN_PRICE = 226
RN_DEPOSIT = 227
RN_EXPIRY = 228
RN_MAKE = 229
RN_MODEL = 230
RN_YEAR = 231
RN_GEARBOX = 232
RN_FUEL = 233
RN_SEATS = 234
RN_COLOR = 235
RN_ROOMS = 236
RN_SLEEPS = 237
RN_SUBTYPE = 238
RN_FEATURES = 239
RN_TRIM = 240
RN_SIZE_M2 = 241
RN_QUANTITY = 248
# §16.18 + board.rs: who wrote a board notice, and what it cost them. A stand's
# write key is the cell name hashed, so anybody can write any slot — these do
# not make a slot somebody's property, they make authorship checkable and
# flooding paid for.
RN_POSTER, RN_SIG, RN_POW = 242, 243, 244
HN_POSTER, HN_SIG, HN_POW = 245, 246, 247
# §16.18.1's freshness beacon: the Monero block a stamp was mined against.
# Without it every other field in the preimage is either the poster's own or a
# floor division of the clock, so the whole of next year could be mined this
# afternoon and the epoch rotation's "paid for again each week" would not be
# true of anybody who had.
RN_BEACON_HEIGHT, RN_BEACON_HASH = 249, 250
HN_BEACON_HEIGHT, HN_BEACON_HASH = 251, 252
# Leading zero bits a notice's proof of work must show — small, because each
# attempt is an Argon2id evaluation rather than a SHA-256 one.
POW_BITS = 8
# And what one attempt costs. Memory-hard on purpose: SHA-256 gave a GPU some
# three orders of magnitude over a phone, which is not a price, it is a
# rounding error.
POW_MEM_KIB, POW_PASSES, POW_LANES = 4096, 1, 1
# How far back a beacon may sit, and how far ahead of a reader's own tip.
BEACON_BLOCKS, BEACON_AHEAD = 720, 2
GEOHASH_ALPHABET = set("0123456789bcdefghjkmnpqrstuvwxyz")
MAX_ATTACHMENT_BYTES = 1_048_576 - 64
MAX_MIME_CHARS, MAX_FILENAME_CHARS = 64, 96
ITEM_DESC, ITEM_AMOUNT = 185, 186
MAX_ITEM_CHARS, MAX_ITEMS = 64, 64
MAX_ADDRESS_CHARS = 128
CARD_PERSONA, CARD_INBOX, CARD_WRITER, CARD_NAME, CARD_EXPIRY = 167, 168, 169, 170, 171
DET_PERSONA, DET_OUTBOX, DET_BUNDLE, DET_NAME = 172, 173, 174, 175
DET_PAYTO = 182
DET_AVATAR, DET_EMAIL, DET_PHONE, DET_SIGNAL, DET_PRONOUNS = 187, 188, 189, 190, 191
DET_CAR_MODEL, DET_CAR_COLOR, DET_PLATE = 210, 211, 212
MAX_CAR_MODEL_CHARS, MAX_CAR_COLOR_CHARS, MAX_PLATE_CHARS = 24, 16, 12
MAX_AVATAR_BYTES = 12 * 1024
MAX_EMAIL_CHARS, MAX_PHONE_DIGITS, MAX_SIGNAL_CHARS = 254, 15, 48


def _email_is_plausible(s):
    """Deliberately stricter than RFC 5322, which admits quoted strings and
    comments no client should be rendering as an identity."""
    if len(s) > MAX_EMAIL_CHARS or any(ch.isspace() or ord(ch) < 0x20 or
                                       unicodedata.category(ch) == "Cf" for ch in s):
        return False
    if s.count("@") != 1:
        return False
    local, domain = s.split("@")
    if not local or not domain:
        return False
    ok_local = set("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._%+-'")
    if (not all(c in ok_local for c in local) or local.startswith(".")
            or local.endswith(".") or ".." in local):
        return False
    if "." not in domain:
        return False
    tld = domain.rsplit(".", 1)[1]
    ok_domain = set("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789.-")
    return (all(c in ok_domain for c in domain) and not domain[0] in ".-"
            and not domain[-1] in ".-" and ".." not in domain
            and len(tld) >= 2 and tld.isalpha() and tld.isascii())


def _phone_is_plausible(s):
    return 0 < len(s) <= MAX_PHONE_DIGITS and s.isdigit() and s.isascii()


def _signal_is_plausible(s):
    if len(s) > MAX_SIGNAL_CHARS or "." not in s:
        return False
    name, _, digits = s.partition(".")
    ok = set("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_")
    return (len(name) >= 3 and all(c in ok for c in name)
            and (name[0].isalpha() or name[0] == "_")
            and len(digits) >= 2 and digits.isdigit() and digits.isascii())


def _avatar_format_is_known(b):
    return (b.startswith(b"\x89PNG\r\n\x1a\n") or b.startswith(b"\xff\xd8\xff")
            or (len(b) > 12 and b[0:4] == b"RIFF" and b[8:12] == b"WEBP"))
HEAD_NEXT = 176


def _body(buf):
    """Decode to a canonical map keyed by field number."""
    v = decode_canonical(buf)
    if v[0] != "map":
        raise Reject("Malformed", "object is not a map")
    return dict(v[1])


def _take(body, key, kind, what):
    if key not in body:
        raise Reject("Malformed", f"missing {what}")
    k, val = body.pop(key)
    if k != kind:
        raise Reject("Malformed", f"{what} has the wrong major type")
    return val


def _opt(body, key, kind):
    """An optional field, type-checked when present. Absent is not a value."""
    if key not in body:
        return None
    k, val = body.pop(key)
    if k != kind:
        raise Reject("Malformed", f"field {key} has the wrong major type")
    return val


def _take_text(body, key, max_chars, what, required):
    """§16.9's text rule.

    Two halves, and the second one is the interesting half:
      - bounded in *characters*, so the bound does not silently shorten a script
        needing more than one byte per character;
      - a present-but-empty field is MALFORMED, because omitting the key is
        already how you say nothing and §18.1 admits one encoding per meaning.
    NFC is enforced one layer down, by the decoder, for all text alike."""
    if key not in body:
        if required:
            raise Reject("Malformed", f"missing {what}")
        return None
    k, val = body.pop(key)
    if k != "text":
        raise Reject("Malformed", f"{what} is not text")
    if val == "":
        raise Reject("Malformed", f"{what} is present but empty")
    if len(val) > max_chars:
        raise Reject("Malformed", f"{what} exceeds {max_chars} characters")
    return val


def _expect_type(body, want, name):
    got = body.pop(0, (None, None))[1]
    if got != OBJECT_TYPE_CODES[want]:
        raise Reject("Malformed", f"object declares type {got}, expected {name}")


def _finish(body):
    if body:
        raise Reject("Malformed", f"unexpected field {sorted(body)[0]}")


def parse_card(buf):
    b = _body(buf)
    _expect_type(b, "CONTACT_OFFER", "CONTACT_OFFER")
    out = {
        "version": _take(b, 1, "uint", "version"),
        "suite": _take(b, 2, "uint", "suite"),
        "persona": _take(b, CARD_PERSONA, "bytes", "persona"),
        "inbox_key": _take_text(b, CARD_INBOX, MAX_RECORD_KEY_CHARS, "inbox key", True),
        "writer_public": _take(b, CARD_WRITER, "bytes", "writer key"),
        "display_name": _take_text(b, CARD_NAME, MAX_DISPLAY_NAME_CHARS,
                                   "display name", False),
        "expiry": _take(b, CARD_EXPIRY, "uint", "expiry"),
    }
    _finish(b)
    return out


def parse_details(buf):
    b = _body(buf)
    _expect_type(b, "CONTACT_ACCEPT", "CONTACT_ACCEPT")
    out = {
        "version": _take(b, 1, "uint", "version"),
        "suite": _take(b, 2, "uint", "suite"),
        "persona": _take(b, DET_PERSONA, "bytes", "persona"),
        "outbox_key": _take_text(b, DET_OUTBOX, MAX_RECORD_KEY_CHARS, "outbox key", True),
        "prekey_bundle": _take(b, DET_BUNDLE, "bytes", "prekey bundle"),
        "display_name": _take_text(b, DET_NAME, MAX_DISPLAY_NAME_CHARS,
                                   "display name", False),
        # Optional: a contact may publish an address so they can be paid without
        # asking first, at the cost of that address being reused.
        "payto": _take_text(b, DET_PAYTO, MAX_ADDRESS_CHARS, "payout address", False),
        # §16.9's profile. All of it optional, all of it validated here rather
        # than at a screen: these render as identity, and a field nobody checks
        # says whatever the sender wants.
        "avatar": b.pop(DET_AVATAR, (None, None))[1],
        "email": _take_text(b, DET_EMAIL, MAX_EMAIL_CHARS, "email", False),
        "phone": _take_text(b, DET_PHONE, MAX_PHONE_DIGITS, "phone", False),
        "signal": _take_text(b, DET_SIGNAL, MAX_SIGNAL_CHARS, "signal", False),
        "pronouns": b.pop(DET_PRONOUNS, (None, None))[1],
        # The car (§15.12): what lets a rider find the right stranger's
        # vehicle. Short plain text, no control characters — these render
        # beside a name.
        "car_model": _take_text(b, DET_CAR_MODEL, MAX_CAR_MODEL_CHARS, "car model", False),
        "car_color": _take_text(b, DET_CAR_COLOR, MAX_CAR_COLOR_CHARS, "car colour", False),
        "plate": _take_text(b, DET_PLATE, MAX_PLATE_CHARS, "plate", False),
    }
    _finish(b)
    if out["avatar"] is not None:
        a = out["avatar"]
        if not a:
            raise Reject("Malformed", "an empty avatar is not an avatar")
        if len(a) > MAX_AVATAR_BYTES:
            raise Reject("Malformed", f"an avatar may be at most {MAX_AVATAR_BYTES} bytes")
        if not _avatar_format_is_known(a):
            raise Reject("Malformed", "an avatar must be PNG, JPEG or WebP")
    if out["email"] is not None and not _email_is_plausible(out["email"]):
        raise Reject("Malformed", "that is not the shape of an email address")
    if out["phone"] is not None and not _phone_is_plausible(out["phone"]):
        raise Reject("Malformed", "a phone number is digits only")
    if out["signal"] is not None and not _signal_is_plausible(out["signal"]):
        raise Reject("Malformed", "a Signal username is name.digits")
    if out["pronouns"] is not None and out["pronouns"] not in (1, 2, 3, 4, 5, 6):
        raise Reject("Malformed", "unknown pronouns code")
    for k in ("car_model", "car_color", "plate"):
        v = out[k]
        if v is not None and (not v or any(ord(c) < 0x20 or ord(c) == 0x7f for c in v)):
            raise Reject("Malformed", f"a {k.replace('_', ' ')} is short plain text")
    return out


def parse_head(buf):
    b = _body(buf)
    _expect_type(b, "LOG_HEAD", "LOG_HEAD")
    out = {
        "version": _take(b, 1, "uint", "version"),
        "suite": _take(b, 2, "uint", "suite"),
        "next_seq": _take(b, HEAD_NEXT, "uint", "next sequence"),
        "bundle": b.pop(HEAD_BUNDLE, (None, None))[1],
        "read": b.pop(HEAD_READ, (None, None))[1],
    }
    # §16.12: the ring size, when other than the default eight — which MUST be
    # encoded by omission, and must leave room for a head plus one slot.
    ring = b.pop(HEAD_RING, (None, None))[1]
    if ring is not None:
        if ring == 8:
            raise Reject("Malformed",
                         "eight is the default ring and is encoded by omitting the field")
        if not (2 <= ring <= 1024):
            raise Reject("Malformed", "a ring is 2..=1024 subkeys")
    out["ring"] = ring
    _finish(b)
    return out


def parse_hail_notice(buf):
    # §16.17: hostile-surface rules, each refused rather than repaired. The
    # card is the only field with teeth; everything else is an untrusted claim.
    b = _body(buf)
    out = {
        "version": _take(b, HN_VERSION, "uint", "version"),
        "card": _take(b, HN_CARD, "text", "card"),
        "dest": _take(b, HN_DEST, "text", "destination"),
        "fare": b.pop(HN_FARE, (None, None))[1],
    }
    # 2, not 1: a version-1 notice carries neither an author nor a proof of
    # work, and there is no safe way to read one.
    if out["version"] != 2:
        raise Reject("Malformed", "unknown hail notice version")
    if not out["card"].startswith("ducat:"):
        raise Reject("Malformed", "a hail card must be a ducat: URI")
    if len(out["card"]) > 1024:
        raise Reject("Malformed", "card too long")
    if not (1 <= len(out["dest"].encode()) <= 64):
        raise Reject("Malformed", "destination is 1..=64 bytes")
    if out["fare"] == 0:
        raise Reject("Malformed", "a zero fare offer is a missing one")
    out["expiry"] = _take(b, HN_EXPIRY, "uint", "expiry")
    out["origin_cell"] = b.pop(HN_ORIGIN_CELL, (None, None))[1]
    out["dest_cell"] = b.pop(HN_DEST_CELL, (None, None))[1]
    for cell in (out["origin_cell"], out["dest_cell"]):
        if cell is None:
            continue
        # §16.17: a board cell is a geohash no finer than precision 6 —
        # ~1.2 km is the floor, by construction.
        if not (1 <= len(cell) <= 6):
            raise Reject("Malformed", "a board cell is 1..=6 characters")
        if not set(cell.lower()) <= GEOHASH_ALPHABET:
            raise Reject("Malformed", "not a geohash")
    _finish(b)
    return out



def parse_listing(buf):
    # §16.18: a listing is an advertisement everyone can read, so the rules
    # are about what it may *not* say as much as what it must.
    b = _body(buf)
    out = {
        "version": _take(b, RN_VERSION, "uint", "version"),
        "card": _take(b, RN_CARD, "text", "card"),
        "kind": _take(b, RN_KIND, "uint", "kind"),
        "title": _take(b, RN_TITLE, "text", "title"),
    }
    if out["version"] != 2:
        raise Reject("Malformed", "unknown rental notice version")
    if not out["card"].startswith("ducat:"):
        raise Reject("Malformed", "a listing card must be a ducat: URI")
    # Five kinds. This knew two, and had known two since draft 0.89 added a
    # thing for sale (3), gear by the day (4) and somebody's time (5) — no
    # vector had ever exercised one, so a second implementation that would
    # have refused every real sale, hire and trade listing on a live board sat
    # here agreeing with itself.
    if out["kind"] not in (1, 2, 3, 4, 5):
        raise Reject("Malformed", "unknown listing kind")
    if not out["title"] or len(out["title"]) > 60:
        raise Reject("Malformed", "a listing needs a title")
    out["area"] = _opt(b, RN_AREA, "text")
    if out["area"] is None:
        out["area"] = ""
    if len(out["area"]) > 40:
        raise Reject("Malformed", "text too long")
    out["cell"] = _opt(b, RN_CELL, "text")
    if out["cell"] is not None:
        # Coarser than a hail by rule: five characters, not six.
        if not (1 <= len(out["cell"]) <= 5):
            raise Reject("Malformed", "a listing cell is 1..=5 characters")
        if not set(out["cell"].lower()) <= GEOHASH_ALPHABET:
            raise Reject("Malformed", "not a geohash")
    out["price"] = _take(b, RN_PRICE, "uint", "price")
    if out["price"] == 0:
        raise Reject("Malformed", "a listing with no price is not an offer")
    out["deposit"] = _opt(b, RN_DEPOSIT, "uint")
    if out["deposit"] is None:
        out["deposit"] = 0
    out["expiry"] = _take(b, RN_EXPIRY, "uint", "expiry")

    for name, fid, typ in [
        ("make", RN_MAKE, "text"), ("model", RN_MODEL, "text"),
        ("color", RN_COLOR, "text"), ("year", RN_YEAR, "uint"),
        ("gearbox", RN_GEARBOX, "uint"), ("fuel", RN_FUEL, "uint"),
        ("seats", RN_SEATS, "uint"), ("rooms", RN_ROOMS, "uint"),
        ("sleeps", RN_SLEEPS, "uint"), ("subtype", RN_SUBTYPE, "uint"),
        ("trim", RN_TRIM, "text"), ("size_m2", RN_SIZE_M2, "uint"),
    ]:
        out[name] = _opt(b, fid, typ)
        if typ == "text" and out[name] is not None and len(out[name]) > 24:
            raise Reject("Malformed", "text too long")

    # A place with a gearbox is describing two things.
    vehicle_only = any(out[k] is not None for k in
                       ("make", "model", "year", "gearbox", "fuel", "seats", "color", "trim"))
    place_only = any(out[k] is not None for k in ("rooms", "sleeps", "size_m2"))
    if out["kind"] == 1 and vehicle_only:
        raise Reject("Malformed", "a place does not have a make, a trim, a gearbox or a fuel")
    if out["kind"] == 2 and place_only:
        raise Reject("Malformed", "a vehicle does not have bedrooms or floor area")
    if out["gearbox"] is not None and not (1 <= out["gearbox"] <= 2):
        raise Reject("Malformed", "gearbox is manual or automatic")
    if out["fuel"] is not None and not (1 <= out["fuel"] <= 4):
        raise Reject("Malformed", "unknown fuel")
    # The three kinds added in 0.89 carry no typed extras at all: a kayak has
    # no gearbox, a bicycle for sale has no bedrooms, an electrician neither.
    if out["kind"] > 2 and (vehicle_only or place_only):
        raise Reject("Malformed", "this listing kind has no typed extras")
    if out["subtype"] is not None:
        top = {1: 2, 2: 3, 3: 9, 4: 5, 5: 12}.get(out["kind"], 0)
        if not (1 <= out["subtype"] <= top):
            raise Reject("Malformed", "unknown subtype")
    if out["year"] is not None and not (1900 <= out["year"] <= 2200):
        raise Reject("Malformed", "implausible year")

    # How many of it there are. One is the absent case and the only spelling
    # of it: a board slot is scarce, and two byte-strings that mean the same
    # listing is the seam the signature is meant to close.
    q = _opt(b, RN_QUANTITY, "uint")
    if q is not None and out["kind"] == 5:
        raise Reject("Malformed", "somebody's time is not stock")
    if q is not None and q <= 1:
        raise Reject("Malformed", "a quantity is written only when it is more than one")
    if q is not None and q > 999:
        raise Reject("Malformed", "more than a listing is for")
    out["quantity"] = q if q is not None else 1

    feats = _opt(b, RN_FEATURES, "array")
    out["features"] = []
    if feats is not None:
        if len(feats) > 8:
            raise Reject("Malformed", "too many features to be a summary")
        for item in feats:
            # Array members arrive typed, as everywhere else in this decoder.
            kind, val = item if isinstance(item, tuple) else (None, item)
            if kind != "text" or not isinstance(val, str) or not val or len(val) > 16:
                raise Reject("Malformed", "a feature is a short word")
            out["features"].append(val)
    _finish(b)
    return out



def leading_zero_bits(h):
    """How many zero bits a digest opens with."""
    n = 0
    for b in h:
        if b:
            return n + (8 - b.bit_length())
        n += 8
    return n


def open_board_notice(buf, board, subkey):
    """§16.18: check who wrote a board notice and that writing it cost something.

    A stand's write key is SHA-256 of the cell name, so every reader of a board
    also holds the key to every slot on it. Nothing here changes that. What the
    seal establishes is narrower, and worth stating precisely:

      * the *bytes* have an author, so a listing copied with somebody else's
        card comes back as a different author rather than as the same one;
      * the signature covers the slot as well as the notice, so a valid one
        cannot be lifted onto another slot — without that, one signed listing
        could paper a whole cell and read as its author flooding the board;
      * a nonce proves the write cost a search, so flooding is no longer free.

    Re-encoding a decoded map to check a signature is normally the wrong
    instinct. It is right here for one reason, and only that one: the codec
    refuses non-canonical input outright, so decoding having succeeded *means*
    the bytes were exactly the canonical encoding of this map.
    """
    import hashlib
    body_map = _body(buf)
    poster = body_map.pop(RN_POSTER, (None, None))[1]
    sig = body_map.pop(RN_SIG, (None, None))[1]
    nonce = body_map.pop(RN_POW, (None, None))[1]
    height = body_map.pop(RN_BEACON_HEIGHT, (None, None))[1]
    bhash = body_map.pop(RN_BEACON_HASH, (None, None))[1]
    if not isinstance(poster, (bytes, bytearray)):
        raise Reject("Malformed", "a board notice must say who wrote it")
    if not isinstance(sig, (bytes, bytearray)) or len(sig) != 64:
        raise Reject("Malformed", "a board notice must be signed")
    if not isinstance(nonce, int):
        raise Reject("Malformed", "a board notice must carry its proof of work")
    if not isinstance(height, int):
        raise Reject("Malformed",
                     "a board notice must name the block it was stamped against")
    if not isinstance(bhash, (bytes, bytearray)):
        raise Reject("Malformed",
                     "a board notice must carry the block hash it was stamped against")
    if len(bhash) != 32:
        raise Reject("Malformed", "a block hash is 32 bytes")

    body = _reencode_map(sorted(body_map.items()))
    # The beacon is inside the signature as well as inside the work. Bound to
    # the work alone, moving it would only force a re-mine; bound to both, a
    # notice names one block and cannot be made to name another — which is what
    # the cheap height test leans on.
    signed = (board.encode() + b"\x00" + int(subkey).to_bytes(4, "little")
              + b"\x00" + int(height).to_bytes(8, "little")
              + b"\x00" + bytes(bhash)
              + b"\x00" + body)
    verify_sig(1, bytes(poster), bytes(sig), sig_input("BOARD_NOTICE", 1, signed))

    # Nonce as the password, the notice folded to sixteen bytes as the salt:
    # Argon2 has no midstate to reuse, so putting the listing through SHA-256
    # once per notice instead of once per attempt is where the saving has to
    # come from.
    salt = hashlib.sha256(
        b"DUCAT-POW-v1" + b"\x00" + signed + b"\x00" + bytes(sig)
    ).digest()[:16]
    from argon2.low_level import hash_secret_raw, Type
    out = hash_secret_raw(
        secret=nonce.to_bytes(8, "little"), salt=salt,
        time_cost=POW_PASSES, memory_cost=POW_MEM_KIB,
        parallelism=POW_LANES, hash_len=32, type=Type.ID,
    )
    if leading_zero_bits(out) < POW_BITS:
        raise Reject("Malformed", "this notice did not pay for its slot")
    return bytes(poster), body


def beacon_in_window(height, tip_height):
    """Is a stamp's block recent enough to have been mined against?

    Height only, and on purpose: it is the free half of the test, run against a
    number every client already has, so the half that costs a lookup is asked
    only of the heights that survive this. Passing it is not freshness — a
    beacon nobody looks up is thirty-two bytes the attacker chose.
    """
    return height <= tip_height + BEACON_AHEAD and height + BEACON_BLOCKS >= tip_height


def run_board_sealed(cases, r):
    for c in cases:
        def go(c=c):
            poster, inner = open_board_notice(
                unhex(c["sealed_hex"]), c["board"], c["subkey"])
            # What is left has to be a listing this implementation reads, so
            # the seal and the notice really do compose.
            parse_listing(inner)
            return poster
        out = expect_reject(r, "contact", c, go)
        want = c["expect"].get("poster_hex")
        if out is not None and want is not None and out.hex() != want:
            r.passed -= 1
            r.bad("contact", c["name"], c.get("why", ""),
                  f"poster {out.hex()}, expected {want}")


def run_beacon_window(cases, r):
    """§16.18.1's freshness window, as a *caller* applies it.

    Judging a beacon needs a chain, and decoding must not consult one, so the
    rule lives here rather than inside the notice reader: a tip of zero is a
    device with no chain view and skips the test entirely — reading a board has
    never required a Monero node — and anything else is the range.
    """
    for c in cases:
        tip = c["tip_height"]
        got = tip == 0 or beacon_in_window(c["beacon_height"], tip)
        want = c["expect"]["ok"]
        if got != want:
            r.bad("contact", c["name"], c.get("why", ""),
                  f"window said {got}, expected {want}")
        else:
            r.passed += 1


def beacon_verdict(height, beacon_hash, tip_height, known_hash):
    """§16.18.1's whole freshness rule — and it has three answers.

    The window is free to check and secures nothing on its own. Monero aims at
    a block every two minutes, so a height months out is predictable to within
    a few hundred: an attacker mines a spread of future heights against block
    hashes they invented, and a reader that stops at the height comparison
    accepts every one of them. So "I cannot check that yet" is `hold`, never
    `show`, and it becomes checkable within minutes.

    A tip of zero is the one case that skips everything — a device with no
    chain view at all, judging the notice on its signature and its work, which
    is what reading a board has always meant.
    """
    if tip_height == 0:
        return "show"
    if not beacon_in_window(height, tip_height):
        return "refuse"
    if known_hash is None:
        return "hold"
    return "show" if known_hash == beacon_hash else "refuse"


def open_position_frame(stream_key, record_key, value):
    """§15.12 — one live-position update, opened.

    A fixed-length XChaCha20-Poly1305 value: 24-byte nonce, then the sealed
    64-byte frame with its 16-byte tag. The record key is the associated data,
    so a value lifted from another ride's record fails to authenticate rather
    than returning someone else's position. Counter monotonicity is the
    reader's, not the frame's — this parses one frame in isolation.
    """
    NONCE, FRAME, TAG = 24, 64, 16
    if len(value) != NONCE + FRAME + TAG:
        raise Reject("Malformed", "a sealed position frame is a fixed length")
    nonce, ct = value[:NONCE], value[NONCE:]
    from Crypto.Cipher import ChaCha20_Poly1305
    cipher = ChaCha20_Poly1305.new(key=stream_key, nonce=nonce)
    cipher.update(record_key.encode())
    try:
        plain = cipher.decrypt_and_verify(ct[:-TAG], ct[-TAG:])
    except ValueError:
        raise Reject("BadSig", "position frame did not authenticate")
    counter = int.from_bytes(plain[0:8], "big")
    lat = int.from_bytes(plain[8:16], "big", signed=True)
    lon = int.from_bytes(plain[16:24], "big", signed=True)
    heading_raw = int.from_bytes(plain[24:26], "big")
    captured = int.from_bytes(plain[26:34], "big")
    if any(x != 0 for x in plain[34:]):
        raise Reject("Malformed", "a position frame's padding must be zero")
    if not (-900_000_000 <= lat <= 900_000_000):
        raise Reject("Malformed", "latitude out of range")
    if not (-1_800_000_000 <= lon <= 1_800_000_000):
        raise Reject("Malformed", "longitude out of range")
    if heading_raw == 0xFFFF:
        heading = None
    elif heading_raw <= 359:
        heading = heading_raw
    else:
        raise Reject("Malformed", "heading is 0..=359 or absent")
    return {"counter": counter, "lat_e7": lat, "lon_e7": lon,
            "heading": heading, "captured": captured}


def run_position_frame(cases, r):
    for c in cases:
        sk = bytes.fromhex(c["stream_key_hex"])
        rec = c["record_key"]
        try:
            got = open_position_frame(sk, rec, unhex(c["frame_sealed_hex"]))
        except Reject as e:
            if c["expect"]["ok"]:
                r.bad("contact", c["name"], c.get("why", ""),
                      f"refused a frame the vector accepts: {e.name}")
            elif e.name.upper() != c["expect"]["reject"].upper():
                r.bad("contact", c["name"], c.get("why", ""),
                      f"refused with {e.name}, vector says {c['expect']['reject']}")
            else:
                r.passed += 1
            continue
        if not c["expect"]["ok"]:
            r.bad("contact", c["name"], c.get("why", ""), "opened a frame the vector refuses")
            continue
        exp = c["expect"]
        want_heading = exp.get("heading")
        if (got["counter"] != exp["counter"] or got["lat_e7"] != exp["lat_e7"]
                or got["lon_e7"] != exp["lon_e7"] or got["captured"] != exp["captured"]
                or got["heading"] != want_heading):
            r.bad("contact", c["name"], c.get("why", ""), f"decoded {got}")
        else:
            r.passed += 1


def run_beacon_verdict(cases, r):
    for c in cases:
        got = beacon_verdict(
            c["verdict_height"], c["beacon_hash"], c["verdict_tip"], c.get("known_hash"),
        )
        want = c["expect"]["verdict"]
        if got != want:
            r.bad("contact", c["name"], c.get("why", ""),
                  f"verdict {got}, expected {want}")
        else:
            r.passed += 1


def run_listing(cases, r):
    for c in cases:
        def go(c=c):
            n = parse_listing(unhex(c["listing_hex"]))
            fields = [
                (RN_VERSION, ("uint", n["version"])),
                (RN_CARD, ("text", n["card"])),
                (RN_KIND, ("uint", n["kind"])),
                (RN_TITLE, ("text", n["title"])),
                (RN_AREA, ("text", n["area"])),
            ]
            if n["cell"] is not None:
                fields.append((RN_CELL, ("text", n["cell"])))
            fields.append((RN_PRICE, ("uint", n["price"])))
            fields.append((RN_DEPOSIT, ("uint", n["deposit"])))
            fields.append((RN_EXPIRY, ("uint", n["expiry"])))
            for name, fid, typ in [
                ("make", RN_MAKE, "text"), ("model", RN_MODEL, "text"),
                ("year", RN_YEAR, "uint"), ("gearbox", RN_GEARBOX, "uint"),
                ("fuel", RN_FUEL, "uint"), ("seats", RN_SEATS, "uint"),
                ("color", RN_COLOR, "text"), ("rooms", RN_ROOMS, "uint"),
                ("sleeps", RN_SLEEPS, "uint"), ("subtype", RN_SUBTYPE, "uint"),
                ("trim", RN_TRIM, "text"), ("size_m2", RN_SIZE_M2, "uint"),
            ]:
                if n[name] is not None:
                    fields.append((fid, (typ, n[name])))
            if n["features"]:
                fields.append((RN_FEATURES, ("array", [("text", f) for f in n["features"]])))
            if n["quantity"] > 1:
                fields.append((RN_QUANTITY, ("uint", n["quantity"])))
            return _reencode_map(fields)
        out = expect_reject(r, "contact", c, go)
        if out is not None and out.hex() != c["expect"]["reencodes_to_hex"]:
            r.passed -= 1
            r.bad("contact", c["name"], c.get("why", ""),
                  f"re-encoded to {out.hex()}, expected {c['expect']['reencodes_to_hex']}")


def run_hail_notice(cases, r):
    for c in cases:
        def go(c=c):
            n = parse_hail_notice(unhex(c["notice_hex"]))
            fields = [
                (HN_VERSION, ("uint", n["version"])),
                (HN_CARD, ("text", n["card"])),
                (HN_DEST, ("text", n["dest"])),
            ]
            if n["fare"] is not None:
                fields.append((HN_FARE, ("uint", n["fare"])))
            fields.append((HN_EXPIRY, ("uint", n["expiry"])))
            if n["origin_cell"] is not None:
                fields.append((HN_ORIGIN_CELL, ("text", n["origin_cell"])))
            if n["dest_cell"] is not None:
                fields.append((HN_DEST_CELL, ("text", n["dest_cell"])))
            return _reencode_map(fields)
        out = expect_reject(r, "contact", c, go)
        if out is not None and out.hex() != c["expect"]["reencodes_to_hex"]:
            r.passed -= 1
            r.bad("contact", c["name"], c.get("why", ""),
                  f"re-encoded to {out.hex()}, vector says "
                  f"{c['expect']['reencodes_to_hex']}")


def parse_message(buf):
    b = _body(buf)
    _expect_type(b, "MESSAGE", "MESSAGE")
    out = {
        "version": _take(b, 1, "uint", "version"),
        "suite": _take(b, 2, "uint", "suite"),
        "seq": _take(b, MSG_SEQ, "uint", "sequence"),
        "prev": _take(b, MSG_PREV, "bytes", "previous link"),
        "body": _take_text(b, MSG_BODY, MAX_MESSAGE_CHARS, "body", True),
        "timestamp": _take(b, MSG_TS, "uint", "timestamp"),
    }
    if len(out["prev"]) != 32:
        raise Reject("Malformed", "previous link is not 32 bytes")

    # §16.13. Text is the default and is encoded by *omitting* the kind — the
    # explicit zero is refused, because one meaning with two encodings is what
    # §18.1 exists to prevent.
    if MSG_KIND in b:
        k, kind = b.pop(MSG_KIND)
        if k != "uint":
            raise Reject("Malformed", "kind is not an integer")
        if kind == 0:
            raise Reject("Malformed", "text is encoded by omitting the kind")
        if kind not in (1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11):
            raise Reject("Malformed", "unknown message kind")
    else:
        kind = 0
    out["kind"] = kind
    out["amount"] = b.pop(MSG_AMOUNT, (None, None))[1]
    out["txid"] = b.pop(MSG_TXID, (None, None))[1]
    out["payto"] = _take_text(b, MSG_PAYTO, MAX_ADDRESS_CHARS, "destination", False)

    # §16.13's itemisation. Absent is "not itemised"; present-but-empty is the
    # same claim spelled a second way, which §18.1 refuses everywhere else.
    out["items"] = []
    if MSG_ITEMS in b:
        k, raw = b.pop(MSG_ITEMS)
        if k != "array":
            raise Reject("Malformed", "items is not an array")
        if not raw:
            raise Reject("Malformed", "an empty item list is not itemisation")
        if len(raw) > MAX_ITEMS:
            raise Reject("Malformed", f"a bill may carry at most {MAX_ITEMS} items")
        for entry in raw:
            ek, emap = entry
            if ek != "map":
                raise Reject("Malformed", "a line item is not a map")
            eb = dict(emap)
            desc = _take_text(eb, ITEM_DESC, MAX_ITEM_CHARS, "item description", True)
            amt = _take(eb, ITEM_AMOUNT, "uint", "item amount")
            _finish(eb)
            out["items"].append({"description": desc, "amount": amt})
    out["tax"] = b.pop(MSG_TAX, (None, None))[1]

    # §16.14 — reactions.
    out["re_seq"] = b.pop(MSG_RE_SEQ, (None, None))[1]
    out["eta"] = b.pop(MSG_ETA, (None, None))[1]
    out["payload"] = b.pop(MSG_PAYLOAD, (None, None))[1]
    out["round"] = b.pop(MSG_ROUND, (None, None))[1]
    out["ceremony"] = b.pop(MSG_CEREMONY, (None, None))[1]
    if MSG_RE_OWN in b:
        k2, v2 = b.pop(MSG_RE_OWN)
        if k2 != "uint" or v2 != 1:
            raise Reject("Malformed", "re_own is a presence flag and may only be 1")
        out["re_own"] = True
    else:
        out["re_own"] = False

    # §16.15 — attachments: all fields or none.
    att = {
        "record": _take_text(b, MSG_ATT_RECORD, MAX_RECORD_KEY_CHARS, "record", False),
        "key": b.pop(MSG_ATT_KEY, (None, None))[1],
        "nonce": b.pop(MSG_ATT_NONCE, (None, None))[1],
        "len": b.pop(MSG_ATT_LEN, (None, None))[1],
        "hash": b.pop(MSG_ATT_HASH, (None, None))[1],
        "mime": _take_text(b, MSG_ATT_MIME, MAX_MIME_CHARS, "mime", False),
        "name": _take_text(b, MSG_ATT_NAME, MAX_FILENAME_CHARS, "filename", False),
    }
    core = [att["record"], att["key"], att["nonce"], att["len"], att["hash"], att["mime"]]
    if all(x is None for x in core):
        if att["name"] is not None:
            raise Reject("Malformed", "a filename without an attachment names nothing")
        out["attachment"] = None
    elif all(x is not None for x in core):
        if att["key"] is not None and len(att["key"]) != 32:
            raise Reject("Malformed", "attachment key is 32 bytes")
        if len(att["nonce"]) != 24:
            raise Reject("Malformed", "attachment nonce is 24 bytes")
        if len(att["hash"]) != 32:
            raise Reject("Malformed", "attachment hash is 32 bytes")
        if not (1 <= att["len"] <= MAX_ATTACHMENT_BYTES):
            raise Reject("Malformed", "attachment length out of bounds")
        out["attachment"] = att
    else:
        raise Reject("Malformed",
                     "an attachment carries record, key, nonce, length, hash and mime together")

    # §15.12 — the live-position reference: both fields together or neither.
    pos_record = _take_text(b, MSG_POS_RECORD, MAX_RECORD_KEY_CHARS, "record", False)
    pos_stream = b.pop(MSG_POS_STREAM, (None, None))[1]
    if pos_record is None and pos_stream is None:
        out["position"] = None
    elif pos_record is not None and pos_stream is not None:
        if len(pos_stream) != 32:
            raise Reject("Malformed", "a stream key is 32 bytes")
        out["position"] = {"record": pos_record, "stream": pos_stream}
    else:
        raise Reject("Malformed",
                     "a position reference carries its record and its key together")
    _finish(b)

    # A payment with no amount is a screen with a blank where the number goes;
    # an amount on text is a number nothing will honour. Neither is ignorable.
    # A FROST round (9) is the exception: a release proposal MAY state the
    # amount the funder gets back — the consent screen shows it beside the
    # signed payload (§15.12's settlement); a statement, not authority.
    if kind in (0, 4, 5, 8, 10) and out["amount"] is not None:
        raise Reject("Malformed", "this kind must not carry an amount")
    if kind in (1, 2, 3) and out["amount"] is None:
        raise Reject("Malformed", "a payment message must carry an amount")
    # §15.12's ceremony: an offer without a fare offers nothing, and an accept
    # must echo the fare so "accepted" is bound to a price both parties said.
    if kind in (6, 7) and out["amount"] is None:
        raise Reject("Malformed", "a ride message must carry the fare")
    # A notice points at the transaction it made; a receipt (§16.13) at the one
    # it acknowledges. A request may point at neither.
    if out["txid"] is not None and kind not in (2, 3):
        raise Reject("Malformed", "only a notice or a receipt carries a transaction")
    # §16.13: a request says where to pay. A notice doing so would be describing
    # a payment it claims to have already made.
    if out["payto"] is not None and kind != 1:
        raise Reject("Malformed", "only a request names where to pay")

    if kind == 4:
        if out["re_seq"] is None:
            raise Reject("Malformed", "a reaction names the message it is about")
        if len(out["body"]) > 16:
            raise Reject("Malformed", "a reaction's body is the emoji, not a message")
        if out["amount"] is not None or out["attachment"] is not None:
            raise Reject("Malformed", "a reaction carries no money and no attachment")
    elif kind in (5, 7):
        # A retract or an accept names its target; an accept answers the
        # *counterparty's* offer, never the sender's own.
        if out["re_seq"] is None:
            raise Reject("Malformed", "a retract or an accept names the message it answers")
        if kind == 7 and out["re_own"]:
            raise Reject("Malformed", "an accept answers the counterparty's offer")
    elif kind in (0, 2, 3):
        # Three kinds *must* name a target (above); three *may* (here); the
        # rest may not. A reply is a text answering a text; a PaymentSent
        # naming the PaymentRequest it settles, and a Receipt naming the
        # request it receipts, state a relationship a reader used to have to
        # infer from the amount — and inferring it was wrong where two
        # identical bills were answered by one payment. Advisory like every
        # other claim in a message: the reference says which request the
        # sender means, not that the money arrived.
        pass
    elif out["re_seq"] is not None or out["re_own"]:
        raise Reject("Malformed", "this kind of message does not target another")
    if out["attachment"] is not None and kind != 0:
        raise Reject("Malformed", "only a text message carries an attachment")
    # A stream reference is a PositionRef's whole content and nothing else's.
    if kind == 11 and out["position"] is None:
        raise Reject("Malformed", "a position message carries a reference to the stream")
    if kind != 11 and out["position"] is not None:
        raise Reject("Malformed", "only a position message carries a stream reference")
    if kind in (0, 5, 6, 7, 8, 9, 10) and (out["items"] or out["tax"] is not None):
        raise Reject("Malformed", "this message kind has no bill to itemise")
    # An eta is a ride offer's courtesy figure, bounded by honesty: a day.
    if out["eta"] is not None:
        if kind != 6:
            raise Reject("Malformed", "only a ride offer carries an eta")
        if out["eta"] > 86_400:
            raise Reject("Malformed", "an eta longer than a day is not an eta")
    # §17.9 ceremony fields ride only on ceremony kinds.
    if kind in (8, 9):
        if not out["payload"]:
            raise Reject("Malformed", "a ceremony round carries a payload")
        if len(out["payload"]) > MAX_ATTACHMENT_BYTES:
            raise Reject("Malformed", "a ceremony payload is bounded like an attachment")
        if out["round"] is None or out["ceremony"] is None:
            raise Reject("Malformed", "a ceremony round names its round and its escrow")
        if out["ceremony"] is not None and len(out["ceremony"]) != 32:
            raise Reject("Malformed", "a ceremony id is 32 bytes")
    elif kind == 10:
        if out["ceremony"] is None:
            raise Reject("Malformed", "an abort names the ceremony it ends")
        if out["payload"] is not None:
            raise Reject("Malformed", "an abort withdraws a ceremony; it carries no round payload")
    elif out["payload"] is not None or out["round"] is not None or out["ceremony"] is not None:
        raise Reject("Malformed", "only a ceremony message carries ceremony fields")
    # Tax only alongside items, so an itemisation is always arithmetic the
    # recipient can check rather than a split they have to believe.
    if out["tax"] is not None and not out["items"]:
        raise Reject("Malformed", "tax needs items to be tax on")
    if out["items"]:
        subtotal = sum(i["amount"] for i in out["items"])
        total = subtotal + (out["tax"] or 0)
        # Python integers do not wrap, so the overflow the Rust side catches
        # with checked_add is caught here by the bound the wire format implies.
        if total >= 2 ** 64:
            raise Reject("Malformed", "item amounts overflow")
        if total != out["amount"]:
            raise Reject("Malformed", "the items and tax do not add up to the amount")
    return out


def subkey_for(seq, subkey_count):
    """§16.12's ring. Subkey 0 is the head, so messages start at 1 and wrap
    without ever landing on it — an off-by-one here overwrites the head with a
    message and loses the whole log rather than one entry."""
    slots = subkey_count - 1
    return (seq % slots) + 1


def still_in_ring(seq, next_seq, subkey_count):
    """Whether a reader can still fetch `seq`. A reader that was away long
    enough has genuinely lost messages and must be able to tell: silently
    showing a thread with a hole in it is §16.10's conversation that did not
    happen."""
    slots = subkey_count - 1
    return seq < next_seq and next_seq - seq <= slots


def check_message(msg, expected_seq, previous_bytes):
    """§16.10. A gap is refused rather than stored around: a thread that
    silently skips a message shows a conversation that did not happen."""
    if msg["seq"] != expected_seq:
        raise Reject("StateViolation",
                     f"expected message {expected_seq}, got {msg['seq']}")
    want = (bytes(32) if previous_bytes is None
            else commit(PURPOSE_SPEC["chain"], previous_bytes))
    if msg["prev"] != want:
        raise Reject("CommitMismatch", "message does not follow the one before it")


def _reencode_map(fields):
    """Re-encode from parsed fields, so agreement is on the *object* rather than
    on having echoed back the bytes handed in."""
    return encode(("map", fields))


def run_contact_card(cases, r):
    for c in cases:
        def go(c=c):
            k = parse_card(unhex(c["card_hex"]))
            m = [(0, ("uint", OBJECT_TYPE_CODES["CONTACT_OFFER"])),
                 (1, ("uint", k["version"])), (2, ("uint", k["suite"])),
                 (CARD_PERSONA, ("bytes", k["persona"])),
                 (CARD_INBOX, ("text", k["inbox_key"])),
                 (CARD_WRITER, ("bytes", k["writer_public"])),
                 (CARD_EXPIRY, ("uint", k["expiry"]))]
            if k["display_name"] is not None:
                m.append((CARD_NAME, ("text", k["display_name"])))
            return _reencode_map(m)
        out = expect_reject(r, "contact", c, go)
        if out is not None and out.hex() != c["expect"]["reencodes_to_hex"]:
            r.passed -= 1
            r.bad("contact", c["name"], c.get("why", ""),
                  f"re-encoded to {out.hex()}, vector says "
                  f"{c['expect']['reencodes_to_hex']}")


def run_contact_details(cases, r):
    for c in cases:
        def go(c=c):
            d = parse_details(unhex(c["details_hex"]))
            m = [(0, ("uint", OBJECT_TYPE_CODES["CONTACT_ACCEPT"])),
                 (1, ("uint", d["version"])), (2, ("uint", d["suite"])),
                 (DET_PERSONA, ("bytes", d["persona"])),
                 (DET_OUTBOX, ("text", d["outbox_key"])),
                 (DET_BUNDLE, ("bytes", d["prekey_bundle"]))]
            if d["display_name"] is not None:
                m.append((DET_NAME, ("text", d["display_name"])))
            if d["payto"] is not None:
                m.append((DET_PAYTO, ("text", d["payto"])))
            if d["avatar"] is not None:
                m.append((DET_AVATAR, ("bytes", d["avatar"])))
            for key, field in (("email", DET_EMAIL), ("phone", DET_PHONE),
                               ("signal", DET_SIGNAL)):
                if d[key] is not None:
                    m.append((field, ("text", d[key])))
            if d["pronouns"] is not None:
                m.append((DET_PRONOUNS, ("uint", d["pronouns"])))
            for key, field in (("car_model", DET_CAR_MODEL),
                               ("car_color", DET_CAR_COLOR), ("plate", DET_PLATE)):
                if d[key] is not None:
                    m.append((field, ("text", d[key])))
            return _reencode_map(m)
        out = expect_reject(r, "contact", c, go)
        if out is not None and out.hex() != c["expect"]["reencodes_to_hex"]:
            r.passed -= 1
            r.bad("contact", c["name"], c.get("why", ""),
                  f"re-encoded to {out.hex()}, vector says "
                  f"{c['expect']['reencodes_to_hex']}")


def run_log_head(cases, r):
    for c in cases:
        def go(c=c):
            h = parse_head(unhex(c["head_hex"]))
            fields = [
                (0, ("uint", OBJECT_TYPE_CODES["LOG_HEAD"])),
                (1, ("uint", h["version"])), (2, ("uint", h["suite"])),
                (HEAD_NEXT, ("uint", h["next_seq"])),
            ]
            if h["bundle"] is not None:
                fields.append((HEAD_BUNDLE, ("bytes", h["bundle"])))
            if h["read"] is not None:
                fields.append((HEAD_READ, ("uint", h["read"])))
            if h["ring"] is not None:
                fields.append((HEAD_RING, ("uint", h["ring"])))
            return _reencode_map(fields)
        out = expect_reject(r, "contact", c, go)
        if out is not None and out.hex() != c["expect"]["reencodes_to_hex"]:
            r.passed -= 1
            r.bad("contact", c["name"], c.get("why", ""),
                  f"re-encoded to {out.hex()}, vector says "
                  f"{c['expect']['reencodes_to_hex']}")


MAX_STAND_SHARDS = 16

def stand_shard_name(base, shard):
    # §15.12's overflow ladder, independently implemented: shard 0 is the bare
    # name, overflow shards are "-<n>" decimal with no padding.
    if not base:
        raise Reject("Malformed", "a stand needs a name")
    if shard >= MAX_STAND_SHARDS:
        raise Reject("Malformed", "the ladder is capped at 16 shards")
    return base if shard == 0 else f"{base}-{shard}"


STAND_EPOCH_SECS = 7 * 24 * 60 * 60


def stand_epoch(now_secs):
    # §15.12's generation. Floor division of a clock the caller supplies —
    # never read here, or a vector would start deciding differently one day.
    return now_secs // STAND_EPOCH_SECS


def stand_epoch_name(base, epoch):
    # "<base>@<epoch>", decimal and unpadded, applied before the shard suffix.
    # Re-stamping is refused rather than folded: a name that already names a
    # generation has one, and stamping it again computes a board nobody else
    # does.
    if not base:
        raise Reject("Malformed", "a stand needs a name")
    if "@" in base:
        raise Reject("Malformed", "that stand name already names a generation")
    return f"{base}@{epoch}"


def run_stand_epoch(cases, r):
    for c in cases:
        def go(c=c):
            got = stand_epoch_name(c["base"], c["epoch"])
            if got != c["expect"]["board"]:
                raise Reject("StateViolation",
                             f"board {got!r}, vector says {c['expect']['board']!r}")
            return None
        expect_reject(r, "contact", c, go)


def run_stand_shard(cases, r):
    for c in cases:
        def go(c=c):
            got = stand_shard_name(c["base"], c["shard"])
            if got != c["expect"]["board"]:
                raise Reject("StateViolation",
                             f"board {got!r}, vector says {c['expect']['board']!r}")
            return None
        expect_reject(r, "contact", c, go)


def run_log_ring(cases, r):
    for c in cases:
        def go(c=c):
            seq, count = c["seq"], c["subkey_count"]
            got = subkey_for(seq, count)
            if got != c["expect"]["subkey"]:
                raise Reject("StateViolation",
                             f"seq {seq} maps to subkey {got}, vector says "
                             f"{c['expect']['subkey']}")
            oldest = c["expect"]["oldest_readable"]
            if not still_in_ring(oldest, seq + 1, count):
                raise Reject("StateViolation", f"seq {oldest} should still be readable")
            if oldest > 0 and still_in_ring(oldest - 1, seq + 1, count):
                raise Reject("StateViolation",
                             f"the ring should have passed seq {oldest - 1}")
            return None
        expect_reject(r, "contact", c, go)


def run_message_payment(cases, r):
    for c in cases:
        def go(c=c):
            m = parse_message(unhex(c["payment_hex"]))
            fields = [(0, ("uint", OBJECT_TYPE_CODES["MESSAGE"])),
                      (1, ("uint", m["version"])), (2, ("uint", m["suite"])),
                      (MSG_SEQ, ("uint", m["seq"])),
                      (MSG_PREV, ("bytes", m["prev"])),
                      (MSG_BODY, ("text", m["body"])),
                      (MSG_TS, ("uint", m["timestamp"]))]
            if m["kind"] != 0:
                fields.append((MSG_KIND, ("uint", m["kind"])))
            if m["amount"] is not None:
                fields.append((MSG_AMOUNT, ("uint", m["amount"])))
            if m["txid"] is not None:
                fields.append((MSG_TXID, ("bytes", m["txid"])))
            if m["payto"] is not None:
                fields.append((MSG_PAYTO, ("text", m["payto"])))
            if m["items"]:
                fields.append((MSG_ITEMS, ("array", [
                    ("map", [(ITEM_DESC, ("text", i["description"])),
                             (ITEM_AMOUNT, ("uint", i["amount"]))])
                    for i in m["items"]
                ])))
            if m["tax"] is not None:
                fields.append((MSG_TAX, ("uint", m["tax"])))
            if m["re_seq"] is not None:
                fields.append((MSG_RE_SEQ, ("uint", m["re_seq"])))
            if m["re_own"]:
                fields.append((MSG_RE_OWN, ("uint", 1)))
            if m.get("eta") is not None:
                fields.append((MSG_ETA, ("uint", m["eta"])))
            if m.get("payload") is not None:
                fields.append((MSG_PAYLOAD, ("bytes", m["payload"])))
            if m.get("round") is not None:
                fields.append((MSG_ROUND, ("uint", m["round"])))
            if m.get("ceremony") is not None:
                fields.append((MSG_CEREMONY, ("bytes", m["ceremony"])))
            a = m["attachment"]
            if a is not None:
                fields.append((MSG_ATT_RECORD, ("text", a["record"])))
                fields.append((MSG_ATT_KEY, ("bytes", a["key"])))
                fields.append((MSG_ATT_NONCE, ("bytes", a["nonce"])))
                fields.append((MSG_ATT_LEN, ("uint", a["len"])))
                fields.append((MSG_ATT_HASH, ("bytes", a["hash"])))
                fields.append((MSG_ATT_MIME, ("text", a["mime"])))
                if a["name"] is not None:
                    fields.append((MSG_ATT_NAME, ("text", a["name"])))
            pos = m.get("position")
            if pos is not None:
                fields.append((MSG_POS_RECORD, ("text", pos["record"])))
                fields.append((MSG_POS_STREAM, ("bytes", pos["stream"])))
            return encode(("map", fields))
        out = expect_reject(r, "contact", c, go)
        if out is not None and out.hex() != c["expect"]["reencodes_to_hex"]:
            r.passed -= 1
            r.bad("contact", c["name"], c.get("why", ""),
                  f"re-encoded to {out.hex()}, vector says "
                  f"{c['expect']['reencodes_to_hex']}")


def run_message_chain(cases, r):
    for c in cases:
        def go(c=c):
            prev_bytes = None
            for i, h in enumerate(c["messages_hex"]):
                raw = unhex(h)
                m = parse_message(raw)
                try:
                    check_message(m, i, prev_bytes)
                except Reject as e:
                    if not c["expect"]["ok"] and i != c["expect"]["fails_at_index"]:
                        raise Reject(e.name, f"failed at index {i}, vector says "
                                            f"{c['expect']['fails_at_index']}")
                    raise
                prev_bytes = raw
            return None
        expect_reject(r, "contact", c, go)


BY_KIND = {
    "contact.card": run_contact_card,
    "contact.details": run_contact_details,
    "log.head": run_log_head,
    "log.ring": run_log_ring,
    "stand.shard": run_stand_shard,
    "stand.epoch": run_stand_epoch,
    "hail.notice": run_hail_notice,
    "rental.listing": run_listing,
    "board.sealed": run_board_sealed,
    "board.beacon_window": run_beacon_window,
    "board.beacon_verdict": run_beacon_verdict,
    "position.frame": run_position_frame,
    "message.payment": run_message_payment,
    "message.chain": run_message_chain,
    "escrow.ceremony": run_escrow_ceremony,
    "escrow.ready": run_escrow_ready,
    "escrow.release": run_escrow_release,
    "bond.check": run_bond_check,
    "slash.check": run_slash_check,
    "object.roundtrip": run_object,
    "codec.decode": run_codec,
    "signing.verify": run_signing_verify,
    "signing.pubkey": run_signing_pubkey,
    "negotiate.select": run_negotiate,
    "commit.purposes": run_commit_purposes,
    "commit.substitution": run_commit_substitution,
    "state.sequence": run_state,
    "transcript.replay": run_transcript_replay,
    "transcript.substitution": run_transcript_substitution,
    "backup.import": run_backup,
}


def main():
    root = Path(__file__).resolve().parent.parent / "vectors" / "v1"
    r = Results()

    # Group every case in the set by kind, then dispatch. File names carry no
    # meaning to a consumer: a case's kind decides how to run it, which is the
    # point of publishing a schema.
    grouped = {}
    total = 0
    for path in sorted(root.glob("*.json")):
        if path.name in ("manifest.json", "schema.json"):
            continue
        for case in json.loads(path.read_text())["cases"]:
            kind = case.get("kind")
            if kind not in BY_KIND:
                print(f"  unknown kind {kind!r} in {path.name} "
                      f"({case.get('name')}) — refusing to guess")
                return 2
            grouped.setdefault(kind, []).append(case)
            total += 1
    for kind, cases in grouped.items():
        BY_KIND[kind](cases, r)

    print(f"\nDUCAT second implementation — {total} vector cases\n")
    print(f"  agreed:        {r.passed}")
    print(f"  disagreements: {len(r.disagreements)}\n")
    for cat, name, why, detail in r.disagreements:
        print(f"  [{cat}] {name}")
        print(f"     {detail}")
        if why:
            print(f"     vector's reason: {why[:150]}")
        print()
    return 0 if not r.disagreements else 1


if __name__ == "__main__":
    sys.exit(main())
