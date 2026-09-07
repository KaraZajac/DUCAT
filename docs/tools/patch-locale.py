#!/usr/bin/env python3
"""Merge a partial translation into one locale, keeping en.json's key order.

    python3 tools/patch-locale.py de patch.json

Used when the English copy changes and only the affected strings need
retranslating — writing a whole 240-key file by hand to change twelve of them
invites transcription errors in the 228 that were already correct.

Key order follows en.json so the files stay diffable against each other, and
build.mjs --check still has the last word on markup and completeness.
"""
import json
import sys
import collections
import os

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def main():
    if len(sys.argv) != 3:
        sys.exit("usage: patch-locale.py <code> <patch.json>")
    code, patch_path = sys.argv[1], sys.argv[2]
    loc = os.path.join(HERE, "locales", f"{code}.json")

    en = json.load(open(os.path.join(HERE, "locales", "en.json")),
                   object_pairs_hook=collections.OrderedDict)
    cur = json.load(open(loc))
    patch = json.load(open(patch_path))

    unknown = [k for k in patch if k not in en]
    if unknown:
        sys.exit(f"ERROR: keys not in en.json: {unknown}")

    cur.update(patch)
    # en.json order first, then anything else (there should be nothing else)
    out = collections.OrderedDict((k, cur[k]) for k in en if k in cur)
    for k in cur:
        if k not in out:
            out[k] = cur[k]

    with open(loc, "w") as fh:
        json.dump(out, fh, ensure_ascii=False, indent=2)
        fh.write("\n")
    missing = [k for k in en if k not in out]
    print(f"  {code}: +{len(patch)} patched, {len(out)}/{len(en)} keys"
          + (f", MISSING {missing[:5]}" if missing else ""))


if __name__ == "__main__":
    main()
