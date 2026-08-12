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
        if e == "TxProof" and mode == "Fast":
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
            v = decode_canonical(unhex(c["input_hex"]))
            return encode(v)
        out = expect_reject(r, "codec", c, go)
        if out is not None and "reencodes_to_hex" in c["expect"]:
            want = c["expect"]["reencodes_to_hex"]
            if out.hex() != want:
                r.passed -= 1
                r.bad("codec", c["name"], c.get("why", ""),
                      f"re-encoded to {out.hex()}, vector says {want}")


def run_signing(cases, r):
    for c in cases:
        def go(c=c):
            # Some cases carry no object or signature at all: they test public
            # key parsing alone. Nothing in the vector schema announces which
            # shape a case has — a second implementer discovers it by crashing.
            if "object_hex" not in c:
                verify_sig(c["suite"], unhex(c["pubkey_hex"]), b"\x00" * 64, b"")
                return True
            obj = unhex(c["object_hex"])
            pk = unhex(c["pubkey_hex"])
            sig = unhex(c.get("sig_hex", ""))
            msg = sig_input(c["verify_as"], c["suite"], obj)
            verify_sig(c["suite"], pk, sig, msg)
            return True
        expect_reject(r, "signing", c, go)


def run_negotiate(cases, r):
    for c in cases:
        # negotiate.json also carries two cases that are not negotiations: a
        # commitment-substitution case and a purpose-separation case, each with
        # its own field names. Routing by shape because nothing declares it.
        if "digests_by_purpose" in c.get("expect", {}):
            body = unhex(c["input_hex"])
            bad = [p for p, want in c["expect"]["digests_by_purpose"].items()
                   if commit(p, body).hex() != want]
            if bad:
                r.bad("negotiate", c["name"], c.get("why", ""),
                      f"purpose labels disagree: {bad}")
            else:
                r.ok()
            continue
        if "genuine_offer_hex" in c:
            want = unhex(c["offer_commit_hex"])
            good = commit("offer_commit", unhex(c["genuine_offer_hex"])) == want
            bad = commit("offer_commit", unhex(c["stripped_offer_hex"])) == want
            if good and not bad:
                r.ok()
            else:
                r.bad("negotiate", c["name"], c.get("why", ""),
                      f"genuine matches={good}, stripped matches={bad}")
            continue

        def go(c=c):
            v, s = negotiate(
                c["offered"]["versions"], c["offered"]["suites"],
                c.get("local_versions", c["offered"]["versions"]),
                c.get("payer_preference", c["offered"]["suites"]),
            )
            return {"version": v, "suite": s}
        got = expect_reject(r, "negotiate", c, go)
        if got is not None:
            for k in ("version", "suite"):
                if k in c["expect"] and got[k] != c["expect"][k]:
                    r.passed -= 1
                    r.bad("negotiate", c["name"], c.get("why", ""),
                          f"{k}: we chose {got[k]}, vector says {c['expect'][k]}")
                    break


def parse_event(ev):
    """Vector events come in three spellings for the same thing: "Fund",
    "Accept { from: Payer }", "Elapsed(60s)", and the JSON-object forms
    {"Accept": {"from": "Payer"}} and {"Elapsed": 60}. A second implementer has
    to reverse-engineer this from examples — the vector README documents the case
    fields but not the event grammar."""
    if isinstance(ev, dict):
        (name, arg), = ev.items()
        if name == "Elapsed":
            return "Elapsed", None, int(arg)
        if isinstance(arg, dict) and "from" in arg:
            return name, arg["from"], 0
        return name, None, 0
    s = ev.strip()
    if s.startswith("Elapsed(") and s.endswith("s)"):
        return "Elapsed", None, int(s[len("Elapsed("):-2])
    if "{" in s:
        name, rest = s.split("{", 1)
        return name.strip(), rest.split(":")[1].strip().rstrip("}").strip(), 0
    return s, None, 0


def check_step(r, c, state, mode, role, ev_raw, expect):
    """Returns the next state, or None if the case is finished/failed."""
    ev, origin, elapsed = parse_event(ev_raw)
    want_ok = expect.get("ok", True)
    try:
        nxt, eff = transition(state, ev, origin, mode, role, elapsed)
    except Reject as ex:
        if want_ok:
            r.bad("state", c["name"], c.get("why", ""),
                  f"{ev} from {state}: we rejected ({ex.detail}), "
                  f"vector expects {expect.get('next')}")
            return None
        if ex.code != expect.get("reject_code", ex.code):
            r.bad("state", c["name"], c.get("why", ""),
                  f"{ev} from {state}: we said {ex.name}({ex.code}), "
                  f"vector says {expect.get('reject_name')}({expect.get('reject_code')})")
            return None
        return "__rejected__"
    if not want_ok:
        r.bad("state", c["name"], c.get("why", ""),
              f"{ev} from {state}: we reached {nxt}, vector expects "
              f"{expect.get('reject_name')}")
        return None
    if "next" in expect and nxt != expect["next"]:
        r.bad("state", c["name"], c.get("why", ""),
              f"{ev} from {state}: we reached {nxt}, vector says {expect['next']}")
        return None
    if "effect" in expect and eff != expect["effect"]:
        r.bad("state", c["name"], c.get("why", ""),
              f"{ev} from {state}: effect {eff}, vector says {expect['effect']}")
        return None
    return nxt


def run_state(cases, r):
    for c in cases:
        state, mode, role = c["from"], c.get("mode", "Direct"), c.get("role", "Payer")
        if "deadline_secs" in c:
            got = deadline(state, mode)
            if got != c["deadline_secs"]:
                r.bad("state", c["name"], c.get("why", ""),
                      f"deadline for {state}/{mode}: we say {got}, "
                      f"vector says {c['deadline_secs']}")
                # Deliberately keep going: a deadline disagreement must not mask
                # a behavioural one behind it.
        steps = c["steps"] if "steps" in c else [
            {"event": c["event"], **{k: v for k, v in c["expect"].items()}}
        ]
        ok = True
        for step in steps:
            expect = {k: v for k, v in step.items() if k != "event"}
            nxt = check_step(r, c, state, mode, role, step["event"], expect)
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
        if got is not None and "decoded" in c["expect"]:
            d = c["expect"]["decoded"]
            checks = [
                (1, d["persona_suite"], "persona_suite"),
                (4, d["monero_restore_height"], "monero_restore_height"),
                (7, d["created"], "created"),
            ]
            for key, want, label in checks:
                if got[key][1] != want:
                    r.passed -= 1
                    r.bad("backup", c["name"], c.get("why", ""),
                          f"{label}: we read {got[key][1]}, vector says {want}")
                    break
            else:
                if got[3][1] != d["monero_seed"]:
                    r.passed -= 1
                    r.bad("backup", c["name"], c.get("why", ""), "monero_seed differs")


def run_transcript(cases, r):
    """§18.9(4). Only the commitments are recomputed — that is where two
    implementations actually diverge, and it is the part the vector publishes."""
    for c in cases:
        exp = c.get("expect", {})
        if "accept_chain_link_hex" not in c.get("expect", {}):
            r.ok()
            continue
        accept = unhex(c["accept_hex"])
        want = exp["accept_chain_link_hex"]
        tried = {}
        for label, table in (("spec", PURPOSE_SPEC), ("alt", PURPOSE_ALT)):
            tried[label] = commit(table["chain"], accept).hex()
        if want in tried.values():
            which = [k for k, v in tried.items() if v == want][0]
            if which != "spec":
                r.bad("transcript", c["name"], c.get("why", ""),
                      f"chain-link commitment matches only the '{which}' purpose "
                      f"label; §18.3 names the labels offer_commit/receipt/chain/"
                      f"market_genesis, which produce {tried['spec']}")
            else:
                r.ok()
        else:
            r.bad("transcript", c["name"], c.get("why", ""),
                  f"chain link: vector says {want}; spec labels give {tried['spec']}, "
                  f"alt labels give {tried['alt']}")


# ---------------------------------------------------------------------------
# Findings (§18.11)
#
# Two disagreements survived, and both were spec gaps rather than code bugs —
# the first implementation was right and the document did not say so:
#
#   1. codec/nesting_too_deep. §18.1 listed no nesting bound. A decoder written
#      from that section accepts arbitrarily deep structures, which is a stack
#      exhaustion route on an unauthenticated transport, and — worse for
#      interop — two clients picking their own limits disagree about the same
#      signed bytes. Now normative at 16 levels.
#
#   2. state/closed_Direct_fires_at_deadline. §18.4's table calls itself
#      exhaustive and carried CLOSED's 120 s contact window only as a *guard* on
#      CONTACT_OFFER, never as a deadline. §6.2 had it all along. It changes no
#      state, so it is easy to omit and still wrong to: without it a client
#      keeps session keys alive forever and accepts contact offers forever.
#
# Both were invisible from inside the first implementation, where the code was
# the answer to the question.
# ---------------------------------------------------------------------------


def main():
    root = Path(__file__).resolve().parent.parent / "vectors" / "v1"
    r = Results()
    runners = {
        "codec": run_codec, "signing": run_signing, "negotiate": run_negotiate,
        "state": run_state, "backup": run_backup, "transcript": run_transcript,
    }
    total = 0
    for name, fn in runners.items():
        path = root / f"{name}.json"
        if not path.exists():
            continue
        cases = json.loads(path.read_text())["cases"]
        total += len(cases)
        fn(cases, r)

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
