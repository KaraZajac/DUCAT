# A second implementation

    python3 conformance/ducat_check.py

Runs `vectors/v1/` against an implementation written from the specification
rather than from `core/`. O21 exists because a vector set validated by one
client encodes that client's bugs as the specification, and the only thing that
resolves it is a second client that reads the same document, reaches its own
conclusions, and disagrees somewhere.

## What this is and is not

**It is not clean-room.** It has the same author as the Rust, and no amount of
discipline makes that equivalent to a stranger reading the document cold. O21 is
therefore **advanced, not closed** — closing it needs someone with no knowledge
of `core/`.

**What it can still catch is real**, and it is the category that actually costs
interoperability:

- places where the spec says something the reference does not do,
- places where the reference does something the spec never says,
- places where the spec admits two readings and the vectors quietly pick one.

None of those are visible from inside the first implementation, because there the
code *is* the answer to the question.

## Result

    104 vector cases
    agreed:        101
    disagreements: 3          <- all three were defects in the spec

After the spec was corrected and one reference behaviour changed:

    agreed:        104
    disagreements: 0

Two disagreements were **omissions** — the Rust was right and the document had not
said so. The third was neither implementation being wrong, which is the one worth
the whole exercise.

### 1. §18.1 had no nesting bound

`codec/nesting_too_deep` expects `MALFORMED` for 17 nested arrays. §18.1 listed
definite-length-only, smallest-form integers, unsigned map keys, bytewise
ordering, no floats, no tags, and NFC text. **No depth limit.** A decoder written
from that section accepts the input.

The word "nesting" did not appear in the document. The bound existed only in the
implementation, and the vector's own hint referred to "the 16-level nesting
bound" as though it were specified.

Two consequences, and the second is the one that is easy to miss. Nesting costs
one byte per level, so a tiny payload can exhaust a recursive decoder's stack —
that is the obvious one. But a limit left to the implementer is *worse* than
none, because two clients choosing 16 and 32 disagree about the same signed
bytes, one accepting an object the other calls malformed. The bound is part of
the wire format. §18.1 now states 16.

### 2. §18.4's exhaustive table was not exhaustive

`state/closed_Direct_fires_at_deadline` expects `CLOSED` to have a 120-second
deadline. §18.4's table — which declares itself "normative and exhaustive" —
carried that 120 seconds only as a *guard* on `CONTACT_OFFER`, never as a
deadline row. §6.2 had it all along, as "Contact window post-RECEIPT".

It changes no state, which is exactly why it went missing and exactly why it
matters: a client that does not hold this deadline keeps session keys alive
indefinitely and accepts contact offers forever. **A deadline whose effect is not
a state change still belongs in a table that calls itself exhaustive.** §18.4 now
lists it.

### 3. Negative integers were unspecified, and both clients were conformant

The reference accepted CBOR major type 1. This implementation refused it, on the
grounds that money is unsigned piconero (§18.2), map keys are unsigned, and every
timestamp is a count — so nothing in the protocol has a use for a signed number.

**Nothing in §18.1 decided the question, so neither was wrong.** Two conformant
clients simply disagreed about whether a byte string was a valid signed object,
and **no conformance suite could have detected it, because there was no correct
answer to test against.**

This is the finding that justifies the exercise. The first two were omissions a
careful reader might have caught. This one was invisible from inside the
reference, where the code *is* the answer to the question, and invisible to the
vector set, which only tests decisions someone already made. It surfaces only
when two implementations read the same text and reach different conclusions.

Resolved toward refusal, because the directions are not symmetric: **later
accepting a value type extends the format; later refusing one breaks every peer
already relying on it.** Strict first is the only reversible choice. §18.1 now
says so, the reference rejects, and two vectors pin it.

## Things a second implementer stumbles over that are not bugs

Recorded because they are friction a real second implementer pays, and because
fixing them is cheap:

- **The vector schema is not documented and is not uniform.** `signing.json`
  cases mostly carry `object_hex` and `sig_hex`; four carry only `pubkey_hex`,
  because they test key parsing alone. Nothing announces which shape a case has,
  so the first encounter is a `KeyError`.
- **`negotiate.json` contains cases that are not negotiations** — one commitment
  substitution and one purpose-separation case, each with its own field names.
- **The state event grammar has three spellings for one thing**: `"Fund"`,
  `"Accept { from: Payer }"`, `"Elapsed(60s)"`, and the JSON forms
  `{"Accept": {"from": "Payer"}}` and `{"Elapsed": 60}`.
- **The transcript vector's prose names purposes `Offer` and `ChainLink`**, which
  are the first implementation's enum names rather than the wire labels §18.3
  specifies. The wire labels are correct — confirmed against
  `commitments_are_domain_separated_by_purpose`, which publishes all four digests
  — but the prose sends a reader looking for the wrong strings.

## Where guesses were made

Marked `GUESS` in the source. Exactly one reached the wire format — negative
integers — and it turned out to be finding 3 rather than a guess: the spec was
silent, so it was a decision this implementation was forced to make and the
reference had made differently. Both are now pinned by vectors.

The remaining marked spots are places where the document is unambiguous and only
the vector *encoding* had to be inferred, which is a documentation gap rather
than a protocol one.
