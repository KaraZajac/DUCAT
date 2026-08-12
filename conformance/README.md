# A second implementation

    python3 conformance/ducat_check.py          # run the vectors
    python3 conformance/validate_vectors.py     # check them against schema.json

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

## Friction that was fixed rather than documented (0.46)

The first pass hit four obstacles that were not protocol bugs — they were the
vector files being neither uniform nor described. Writing a schema that *documented*
three spellings of one event would have formalised the mess, so the format was
normalised and then specified in `vectors/v1/schema.json`. Every case now carries
a `kind`, and `kind` is the only discriminator a consumer needs.

What was wrong, and what it is now:

- **`signing.json` had two shapes with nothing announcing which.** Most cases
  carry `object_hex` and `sig_hex`; four carry only `pubkey_hex`, testing key
  parsing alone. The first encounter was a `KeyError`. Now `signing.verify` and
  `signing.pubkey`.
- **`negotiate.json` contained cases that were not negotiations** — a commitment
  substitution and a purpose-separation case, each with its own field names. Now
  `commit.json`, kinds `commit.substitution` and `commit.purposes`.
- **`transcript.json` contained a case that was not a transcript** — an offer
  substituted after the tap, carrying neither a tap object nor a receipt. Now
  `transcript.substitution`, and `transcript.replay` requires all four objects.
- **The state event grammar had five spellings of one concept**: `"Fund"`,
  `"Accept { from: Payer }"`, `"Elapsed(60s)"`, `{"Accept": {"from": "Payer"}}`,
  `{"Elapsed": 60}`. Now one: `{"name": …, "from": …?, "elapsed_s": …?}`.

Two more that the schema itself caught on its first run, which is the argument for
hand-writing it rather than generating it:

- **A negotiation case never said which versions the local client supported.**
  A negotiation is a function of three inputs; a case omitting one forces the
  consumer to invent a default, which is precisely how two implementations
  diverge. All three are now required.
- **`state.sequence` assertions were split between the case and its steps** —
  some cases put `next`/`effect` beside the event, others under `expect`. Now
  always under `expect`, and a successful transition must assert both, because a
  case checking only the state passes while a client emits the wrong evidence —
  and §6.2 has two unilateral receipts that assert opposite things.

**The transcript prose still names commitment purposes `Offer` and `ChainLink`**,
which are the reference's internal enum names rather than the wire labels §18.3
specifies. The labels are correct — confirmed against
`commitments_are_domain_separated_by_purpose`, which publishes all four digests —
but the prose sends a reader looking for the wrong strings. Left as prose because
it is non-normative; noted because it cost time.

## Where guesses were made

Marked `GUESS` in the source. Exactly one reached the wire format — negative
integers — and it turned out to be finding 3 rather than a guess: the spec was
silent, so it was a decision this implementation was forced to make and the
reference had made differently. Both are now pinned by vectors.

The remaining marked spots are places where the document is unambiguous and only
the vector *encoding* had to be inferred, which is a documentation gap rather
than a protocol one.
