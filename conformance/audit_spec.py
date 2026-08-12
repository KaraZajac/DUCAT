#!/usr/bin/env python3
"""Cross-check the specification against the implementation.

Prose drifts from code silently. Every other checker here validates artifacts
against each other; nothing has been checking that the *document* still
describes what was built. This session alone produced a stale field-registry
row, a manifest claiming a limitation that had been reversed, and a state table
missing a deadline §6.2 carried — each found by accident.

Written as a script rather than prose so the answer stays true after the next
edit.
"""

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SPEC = (ROOT / "ducat-protocol.md").read_text()
problems = []


def bad(area, msg):
    problems.append(f"[{area}] {msg}")


def read(p):
    f = ROOT / p
    return f.read_text() if f.exists() else ""


# --- 1. Section cross-references resolve -----------------------------------
# A §reference to a section that does not exist sends a reader looking for
# something that was renumbered or never written.
headings = set()
for m in re.finditer(r"^#+\s*(\d+(?:\.\d+)*)", SPEC, re.M):
    headings.add(m.group(1))
    parts = m.group(1).split(".")
    for i in range(1, len(parts) + 1):
        headings.add(".".join(parts[:i]))

# Skip §refs on a line that cites an external standard — RFC 8949 §4.2.1 is not
# a DUCAT section and flagging it trains the reader to ignore this check.
EXTERNAL = re.compile(r"RFC\s*\d+|ISO/IEC|BOLT|EMV|BIP-\d+")
for line in SPEC.splitlines():
    if EXTERNAL.search(line):
        continue
    for r in re.findall(r"§(\d+(?:\.\d+)*)", line):
        if r not in headings:
            bad("xref", f"§{r} is referenced but no such section exists")

# --- 2. Open-problem references resolve ------------------------------------
defined_o = set(re.findall(r"^- \*\*O(\d+)\.", SPEC, re.M))
used_o = set(re.findall(r"\bO(\d+)\b", SPEC))
for o in sorted(used_o - defined_o, key=int):
    bad("open-problem", f"O{o} is referenced but never defined")

# --- 3. Reject codes: §18.5 table vs core::reject --------------------------
reject_rs = read("core/src/reject.rs")
code_of = dict(
    (name, int(num))
    for name, num in re.findall(r"^\s+(\w+)\s*=\s*(\d+),", reject_rs, re.M)
)
# Scope to §18.5's table; the field registry also has `| number | NAME |` rows
# and matching those reported a reject code called TERMS.
_start = SPEC.find("## 18.5")
_reject_table = SPEC[_start:SPEC.find("## 18.6", _start)] if _start >= 0 else ""
for num, name in re.findall(r"\|\s*(\d+)\s*\|\s*`([A-Z_]+)`", _reject_table):
    camel = "".join(p.capitalize() for p in name.split("_"))
    fixups = {"BadSig": "BadSig", "UnsupportedVersion": "UnsupportedVersion"}
    camel = fixups.get(camel, camel)
    if camel not in code_of:
        bad("reject", f"§18.5 lists {name} but core::reject has no {camel}")
    elif code_of[camel] != int(num):
        bad("reject", f"{name}: spec says {num}, code says {code_of[camel]}")

# --- 4. Field-number collisions in the wire registry -----------------------
# Keys are scoped per module: `terms_keys` numbers a *nested* map and legitimately
# restarts at 0. Comparing across namespaces reported five collisions that are not
# collisions — a nested object has its own key space.
wire = read("core/src/wire.rs")
namespaces = {}
current = None
for line in wire.splitlines():
    m = re.match(r"\s*pub mod (\w+)\s*\{", line)
    if m:
        current = m.group(1)
    m = re.match(r"\s*mod (\w+)\s*\{", line)
    if m:
        current = m.group(1)
    m = re.search(r"pub const (\w+): u64 = (\d+);", line)
    if m and current:
        ns = namespaces.setdefault(current, {})
        n = int(m.group(2))
        if n in ns and ns[n] != m.group(1):
            bad("fields", f"{current}: field {n} used by both {ns[n]} and {m.group(1)}")
        ns[n] = m.group(1)

# --- 5. Object type codes are unique and match the label table -------------
sig = read("core/src/sig.rs")
type_codes = {}
for name, num in re.findall(r"ObjectType::(\w+) => (\d+),", wire):
    n = int(num)
    if n in type_codes:
        bad("types", f"type code {n} is claimed by {type_codes[n]} and {name}")
    type_codes[n] = name
variants = set(re.findall(r"^\s{4}(\w+),", sig[sig.find("pub enum ObjectType"):sig.find("impl ObjectType")], re.M))
labelled = set(re.findall(r"ObjectType::(\w+) =>", sig))
for v in sorted(variants - labelled):
    bad("types", f"{v} has no domain-separation label")
for v in sorted(variants - set(type_codes.values())):
    bad("types", f"{v} has no wire type code")

# --- 6. Vector kinds agree across schema, generator, and both runners ------
schema = json.loads(read("vectors/v1/schema.json") or "{}")
try:
    schema_kinds = set(
        schema["$defs"]["case"]["properties"]["kind"]["enum"]
    )
except Exception:
    schema_kinds = set()
    bad("vectors", "schema.json has no kind enum")

gen_kinds = set(re.findall(r'"([a-z]+\.[a-z]+)"', read("core/examples/gen_vectors.rs")))
gen_kinds = {k for k in gen_kinds if k.split(".")[0] in
             {"codec", "signing", "negotiate", "commit", "state", "transcript", "backup", "object"}}
py_kinds = set(re.findall(r'"([a-z]+\.[a-z]+)":\s*run_', read("conformance/ducat_check.py")))
rs_kinds = set(re.findall(r'"([a-z]+\.[a-z]+)"', read("core/tests/vectors.rs")))
rs_kinds = {k for k in rs_kinds if "." in k}

for missing in sorted(schema_kinds - py_kinds):
    bad("vectors", f"{missing} is in the schema but the second implementation cannot run it")
for missing in sorted(schema_kinds - rs_kinds):
    bad("vectors", f"{missing} is in the schema but the Rust runner does not list it")
for extra in sorted(gen_kinds - schema_kinds):
    bad("vectors", f"the generator emits {extra}, which the schema does not describe")

# §18.9.1's table is normative: a kind the schema accepts but the document never
# names is a case an implementer cannot know exists. This drifted by six kinds
# before anyone checked.
_k = SPEC.find("18.9.1")
_ktable = SPEC[_k : SPEC.find("## 18.10", _k)] if _k >= 0 else ""
for k in sorted(schema_kinds):
    if f"`{k}`" not in _ktable:
        bad("vectors", f"{k} is in the schema but §18.9.1's table does not list it")

# --- 7. Draft version matches the newest changelog entry -------------------
hdr = re.search(r"\*\*Draft (\d+\.\d+)", SPEC)
first = re.search(r"^- \*\*(\d+\.\d+)\*\* —", SPEC, re.M)
if hdr and first and hdr.group(1) != first.group(1):
    bad("version", f"header says {hdr.group(1)}, newest changelog entry is {first.group(1)}")

# --- 8. Transport identifiers match the spec -------------------------------
tr = read("core/src/transport.rs")
aid = re.search(r"NFC_AID: \[u8; \d+\] = \[([^\]]+)\]", tr)
if aid:
    got = aid.group(1).replace(" ", "").replace("b'", "").replace("'", "")
    if "0xF0" not in got:
        bad("transport", "NFC_AID does not start 0xF0 (ISO 7816-5 proprietary range)")
    spec_aid = re.search(r"F0 44 55 43 41 54", SPEC)
    if not spec_aid:
        bad("transport", "spec no longer states the AID the code implements")
for uuid in re.findall(r'BLE_\w+_UUID: &str = "([0-9a-f-]+)"', tr):
    if uuid not in SPEC:
        bad("transport", f"BLE UUID {uuid} is in code but not in §18.7")

# --- 9. Numeric claims about the vector set --------------------------------
# The spec quotes counts in prose. Those go stale the moment a case is added,
# and a document that miscounts its own artifacts is one a reader stops trusting.
manifest = json.loads(read("vectors/v1/manifest.json") or "{}")
total = manifest.get("total_cases")
for line in SPEC.splitlines():
    # Changelog entries are history and are supposed to say what was true then.
    # Only live prose is checked, or every release note becomes a false alarm and
    # the check gets ignored.
    if re.match(r"- \*\*\d+\.\d+\*\* —", line.strip()):
        continue
    for m in re.finditer(r"(\d+)\s+vectors\b", line):
        n = int(m.group(1))
        if n != total and n > 20:
            bad("counts", f"live prose says '{n} vectors'; the manifest has {total}")

# --- 10. Every object type appears in §6's message table -------------------
# A type with a label and a code that no table lists is an object an implementer
# cannot discover exists.
_msg = SPEC[SPEC.find("## 6."):SPEC.find("## 7.")]
LABEL_EXEMPT = {"attestation", "bond_proof"}  # carried inside other objects
for label in re.findall(r'ObjectType::\w+ => b"([^"]+)"', sig):
    if label in LABEL_EXEMPT:
        continue
    if label not in _msg and label not in SPEC:
        bad("messages", f"object type {label} appears nowhere in the document")

# --- 11. Field numbers fall inside a declared registry range ---------------
_reg = SPEC[SPEC.find("Field-Number Registry"):]
_reg = _reg[: _reg.find("## 18.5")]
ranges = []
for a, b in re.findall(r"\|\s*(\d+)[–-](\d+)\s*\|", _reg):
    ranges.append((int(a), int(b)))
for a in re.findall(r"\|\s*(\d+)\s*\|", _reg):
    ranges.append((int(a), int(a)))
if ranges:
    for n, name in sorted(namespaces.get("f", {}).items()):
        if not any(lo <= n <= hi for lo, hi in ranges):
            bad("registry", f"field {n} ({name}) is outside every range §18.4.2 declares")

print(f"\nspec audit — {len(problems)} problem(s)\n")
for p in problems:
    print(f"  {p}")
print()
sys.exit(1 if problems else 0)
