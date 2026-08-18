#!/usr/bin/env python3
"""The translations, checked the ways they actually break.

Three failures, all of which have happened here:

1. **A placeholder that does not match the base language.** `%1$s` in English
   and `%s` in Polish is a crash at runtime, in a language the author cannot
   read, on a screen they will never open.

2. **Text in the wrong script.** Writing twenty translations in one sitting,
   a phrase from the language above can bleed into the one below — Russian
   words ended up inside the Japanese onboarding copy, and the build was
   perfectly happy about it.

3. **A plural class the language does not use**, or a missing `other`, which
   CLDR requires of everyone.

    python3 applications/check_strings.py
"""
import glob
import os
import re
import sys
import xml.etree.ElementTree as ET

RES = os.path.join(os.path.dirname(os.path.abspath(__file__)), "android/src/main/res")

# The script each language writes in. Languages sharing a script are grouped,
# because "Ukrainian contains Cyrillic" is not a finding.
SCRIPTS = {
    "ru": "cyrillic", "uk": "cyrillic",
    "zh": "han", "ja": "kana-han", "ko": "hangul",
    "ar": "arabic", "fa": "arabic",
    "hi": "devanagari", "th": "thai",
}
RANGES = {
    "cyrillic": r"Ѐ-ӿ",
    "han": r"一-鿿",
    "kana": r"぀-ヿ",
    "hangul": r"가-힯",
    "arabic": r"؀-ۿ",
    "devanagari": r"ऀ-ॿ",
    "thai": r"฀-๿",
}
# What a language may legitimately contain besides its own.
ALLOWED = {
    "kana-han": {"kana", "han"},
    "han": {"han"},
    "cyrillic": {"cyrillic"},
    "hangul": {"hangul", "han"},
    "arabic": {"arabic"},
    "devanagari": {"devanagari"},
    "thai": {"thai"},
}
PLACEHOLDER = re.compile(r"%[0-9]+\$[sd]|%[sd]")


def text_of(node) -> str:
    return "".join(node.itertext())


def read(path):
    """name -> text for strings; (name, quantity) -> text for plural items."""
    out, plurals = {}, {}
    root = ET.parse(path).getroot()
    for s in root.iter("string"):
        if s.get("name"):
            out[s.get("name")] = text_of(s)
    for p in root.iter("plurals"):
        for item in p.iter("item"):
            plurals[(p.get("name"), item.get("quantity"))] = text_of(item)
    return out, plurals


def main() -> int:
    problems = 0
    base_files = sorted(glob.glob(os.path.join(RES, "values/strings_*.xml")))
    locales = sorted(
        d.split("values-")[1]
        for d in glob.glob(os.path.join(RES, "values-*"))
        if os.path.isdir(d)
    )

    for base_path in base_files:
        name = os.path.basename(base_path)
        base, base_plurals = read(base_path)
        for loc in locales:
            path = os.path.join(RES, f"values-{loc}", name)
            if not os.path.isfile(path):
                continue
            strings, plurals = read(path)

            # 1. Placeholders must match the base exactly, as a multiset.
            for key, value in strings.items():
                if key not in base:
                    continue
                want = sorted(PLACEHOLDER.findall(base[key]))
                got = sorted(PLACEHOLDER.findall(value))
                if want != got:
                    print(f"  ! {loc}/{name}: {key} has {got}, base has {want}")
                    problems += 1

            # 2. The text must be written in the language's own script.
            family = SCRIPTS.get(loc)
            if family:
                allowed = ALLOWED[family]
                for key, value in strings.items():
                    for script, rng in RANGES.items():
                        if script in allowed:
                            continue
                        # Two or more adjacent characters: a stray sign or
                        # borrowed brand name is not what this is looking for.
                        hits = re.findall(f"[{rng}]{{2,}}", value)
                        if hits:
                            print(
                                f"  ! {loc}/{name}: {key} contains {script} "
                                f"text: {hits[:2]}",
                            )
                            problems += 1

            # 3. Plurals: 'other' is required of every language, and a class
            #    the language does not use is dead text.
            names = {n for (n, _) in plurals}
            for n in names:
                if (n, "other") not in plurals:
                    print(f"  ! {loc}/{name}: plural {n} has no 'other'")
                    problems += 1
            for (n, quantity), value in plurals.items():
                want = sorted(PLACEHOLDER.findall(base_plurals.get((n, "other"), "")))
                got = sorted(PLACEHOLDER.findall(value))
                if base_plurals and want and not set(got) <= set(want):
                    print(f"  ! {loc}/{name}: plural {n}/{quantity} has {got}, base has {want}")
                    problems += 1

    counted = len(base_files)
    if problems:
        print(f"strings — {problems} problem(s) across {counted} files, {len(locales)} languages")
        return 1
    print(f"strings — {counted} files × {len(locales)} languages agree")
    return 0


if __name__ == "__main__":
    sys.exit(main())
