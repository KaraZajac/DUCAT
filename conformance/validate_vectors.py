#!/usr/bin/env python3
"""Validate every published vector file against vectors/v1/schema.json.

The schema is hand-written and the vectors are generated, so this is a real
cross-check rather than a program agreeing with itself. If the generator starts
emitting a shape the schema does not describe, one of the two is wrong and a
third-party implementer would have found out the hard way.

Also enforces two things JSON Schema cannot express:
  - case names are unique across the whole set, since implementers report
    failures by name and a duplicate makes a report ambiguous;
  - the manifest's counts match the files, so a stale manifest cannot claim
    coverage that is not there.
"""

import json
import sys
from pathlib import Path

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parent.parent / "vectors" / "v1"


def main():
    schema = json.loads((ROOT / "schema.json").read_text())
    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema)

    manifest = json.loads((ROOT / "manifest.json").read_text())
    files = sorted(p for p in ROOT.glob("*.json")
                   if p.name not in ("manifest.json", "schema.json"))

    problems, seen, total = [], {}, 0
    for path in files:
        doc = json.loads(path.read_text())
        for err in validator.iter_errors(doc):
            where = "/".join(str(x) for x in err.absolute_path)
            problems.append(f"{path.name}:{where}: {err.message}")
        for c in doc.get("cases", []):
            total += 1
            name = c.get("name")
            if name in seen:
                problems.append(
                    f"duplicate case name {name!r} in {path.name} and {seen[name]}")
            seen[name] = path.name
        n = len(doc.get("cases", []))
        claimed = manifest["counts"].get(path.stem)
        if claimed != n:
            problems.append(
                f"manifest claims {claimed} cases for {path.stem}, file has {n}")

    if manifest.get("total_cases") != total:
        problems.append(
            f"manifest total_cases={manifest.get('total_cases')}, counted {total}")

    print(f"\nvector schema validation — {len(files)} files, {total} cases\n")
    if problems:
        for p in problems[:40]:
            print(f"  {p}")
        if len(problems) > 40:
            print(f"  … and {len(problems) - 40} more")
        print(f"\n  {len(problems)} problem(s)\n")
        return 1
    print("  all cases validate; names unique; manifest counts match\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
