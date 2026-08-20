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
        # translatable="false" is the author saying this one is a brand name
        # or a bare format string. Comparing those to a translation that will
        # never exist is how a checker earns the right to be ignored.
        if s.get("name") and s.get("translatable") != "false":
            out[s.get("name")] = text_of(s)
    for p in root.iter("plurals"):
        for item in p.iter("item"):
            plurals[(p.get("name"), item.get("quantity"))] = text_of(item)
    return out, plurals


def main() -> int:
    problems = 0
    # `strings*.xml`, not `strings_*.xml`. The underscore excluded exactly one
    # file — `values/strings.xml`, the app's oldest and least per-screen one —
    # so forty strings had never been parity-checked in any language, and a
    # new one added there was validated by nothing at all.
    base_files = sorted(glob.glob(os.path.join(RES, "values/strings*.xml")))
    # Language folders only. `values-v29` and friends are API qualifiers, not
    # locales — counting one as a language makes this report every string in
    # the app as missing from it, which is a lot of noise for a themes file.
    locales = sorted(
        d.split("values-")[1]
        for d in glob.glob(os.path.join(RES, "values-*"))
        if os.path.isdir(d) and not re.fullmatch(r"v\d+", d.split("values-")[1])
    )

    for base_path in base_files:
        name = os.path.basename(base_path)
        base, base_plurals = read(base_path)
        for loc in locales:
            path = os.path.join(RES, f"values-{loc}", name)
            if not os.path.isfile(path):
                # A file no locale has is a whole feature nobody translated,
                # and skipping quietly is how §16.18's screens shipped in
                # English to nineteen languages while this script reported
                # that everything agreed. Absence is the finding.
                print(f"  ! {loc}/{name}: no translation at all")
                problems += 1
                continue
            strings, plurals = read(path)

            # 0. Every translatable string the base has, this locale has.
            #
            #    The file existing is not the same as the file being current.
            #    Eleven strings — the whole PIN step and the renting shell —
            #    were added to values/ alone and shipped in English to every
            #    one of these languages, while this script compared the keys
            #    the two files had in common and reported that they agreed.
            absent = [k for k in base if k not in strings]
            if absent:
                shown = ", ".join(sorted(absent)[:4])
                more = f" (+{len(absent) - 4} more)" if len(absent) > 4 else ""
                print(f"  ! {loc}/{name}: untranslated: {shown}{more}")
                problems += len(absent)

            # 1. Placeholders must match the base exactly, as a multiset.
            for key, value in strings.items():
                if key not in base:
                    continue
                want = sorted(PLACEHOLDER.findall(base[key]))
                got = sorted(PLACEHOLDER.findall(value))
                if want != got:
                    print(f"  ! {loc}/{name}: {key} has {got}, base has {want}")
                    problems += 1

            # 1b. An apostrophe must be escaped in an Android resource.
            #
            #     The base language too, not only the translations. This
            #     checked `strings` and not `base`, so it caught nineteen
            #     Ukrainian apostrophes and missed the English one directly
            #     above them — a check that only looks where you were already
            #     careful is not a check.
            #
            #     Only aapt knew this, and it says so as "Invalid unicode
            #     escape sequence" pointing at the whole file — which is a
            #     long way from "Ukrainian spells зв'язатися with one". The
            #     languages that need it most are the ones where the mark is
            #     a letter rather than punctuation.
            for where, table in (("values", base), (loc, strings)):
                for key, value in table.items():
                    if value.startswith('"'):
                        continue
                    for i, ch in enumerate(value):
                        if ch == "'" and (i == 0 or value[i - 1] != "\\"):
                            print(f"  ! {where}/{name}: {key} has an unescaped apostrophe")
                            problems += 1
                            break

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

    # --- keys nothing displays ------------------------------------------
    #
    # Parity only proves the languages agree with each other. A key no screen
    # references still passes every check above, in all nineteen — so a screen
    # that gets rewritten leaves a full set of translations behind it, and the
    # next person to read the file cannot tell live copy from a fossil. Seven
    # had accumulated before this check existed.
    #
    # Safe to be strict about, because nothing here builds a name at runtime:
    # every reference is a literal `R.string.x` in Kotlin or `@string/x` in
    # XML, and there is no `getIdentifier` anywhere in the tree.
    app = os.path.dirname(os.path.abspath(__file__))
    used = set()
    for root in (
        os.path.join(app, "android/src/main/java"),
        os.path.join(app, "desktop/src/main/kotlin"),
    ):
        for dirpath, _, filenames in os.walk(root):
            for fn in filenames:
                if not fn.endswith(".kt"):
                    continue
                with open(os.path.join(dirpath, fn), encoding="utf-8") as fh:
                    used |= set(re.findall(r"R\.(?:string|plurals)\.(\w+)", fh.read()))
    for dirpath, _, filenames in os.walk(os.path.join(app, "android/src/main")):
        if os.path.join("res", "values") in dirpath:
            continue
        for fn in filenames:
            if not fn.endswith(".xml"):
                continue
            with open(os.path.join(dirpath, fn), encoding="utf-8") as fh:
                used |= set(re.findall(r"@(?:string|plurals)/(\w+)", fh.read()))
    declared = set()
    for path in base_files:
        with open(path, encoding="utf-8") as fh:
            declared |= set(re.findall(r'<(?:string|plurals) name="([^"]+)"', fh.read()))
    for name in sorted(declared - used):
        print(f"  ! values/{name}: nothing references it")
        problems += 1

    # --- strings called without the arguments they take --------------------
    #
    # A string with a `%1$s` in it and no argument at the call site does not
    # fail to build, does not warn, and does not throw. It renders the
    # placeholder, on screen, to a person: "They pay %1$s — funding your
    # deposit accepts it." That shipped for exactly as long as it took to look
    # at the screen, because the *other* rendering of the same moment passed
    # its argument correctly and looked fine.
    #
    # Only the no-argument form is checked. Getting the count wrong is a
    # different mistake and a rarer one; getting it to zero is what a
    # find-and-replace does when a string grows an argument later.
    takes_args = {}
    for path in base_files:
        with open(path, encoding="utf-8") as fh:
            for m in re.finditer(r'<string name="([^"]+)">(.*?)</string>', fh.read(), re.S):
                if re.search(r"%\d\$", m.group(2)):
                    takes_args[m.group(1)] = True
    bare = re.compile(r"(?:stringResource|getString)\(\s*R\.string\.(\w+)\s*\)")
    for dirpath, _, filenames in os.walk(os.path.join(app, "android/src/main/java")):
        for fn in filenames:
            if not fn.endswith(".kt"):
                continue
            full = os.path.join(dirpath, fn)
            with open(full, encoding="utf-8") as fh:
                text = fh.read()
            for m in bare.finditer(text):
                if m.group(1) in takes_args:
                    line = text[: m.start()].count("\n") + 1
                    print(f"  ! {fn}:{line}: {m.group(1)} takes an argument and got none")
                    problems += 1

    counted = len(base_files)
    if problems:
        print(f"strings — {problems} problem(s) across {counted} files, {len(locales)} languages")
        return 1
    print(f"strings — {counted} files × {len(locales)} languages agree")
    return 0


if __name__ == "__main__":
    sys.exit(main())
