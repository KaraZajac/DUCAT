#!/usr/bin/env python3
"""The desk's dictionaries, carried over from the phone.

The phone's screens are translated into nineteen languages under
`applications/android/src/main/res/values-*/strings_*.xml`, checked by
`applications/check_strings.py`. The desk says most of the same things, so
it reads the same files: every key a desk page uses through `t("key")` is
looked up there first, and only sentences the phone never says live in the
desk's own `applications/desk/strings/values*/strings_desk.xml`, kept in
the same format so the same checker can read them.

    python3 applications/desk/scripts/strings_from_android.py          # write src/lib/strings/*.json
    python3 applications/desk/scripts/strings_from_android.py --check  # fail if the JSON is stale or a key is unknown

The JSON carries only the keys the pages use, so a locale costs a few
kilobytes, and English is complete by construction: a key the pages use
that no English file defines fails the check — that is a typo in a page,
found before a reader in Japanese finds it.
"""
import glob
import json
import os
import re
import sys
import xml.etree.ElementTree as ET

HERE = os.path.dirname(os.path.abspath(__file__))
DESK = os.path.dirname(HERE)
ANDROID_RES = os.path.join(DESK, "..", "android", "src", "main", "res")
DESK_RES = os.path.join(DESK, "strings")
OUT = os.path.join(DESK, "src", "lib", "strings")
SRC = os.path.join(DESK, "src")

# t("key"), t('key'), tp("key", n) — the pages' only door to a string.
USED = re.compile(r"\bt[pn]?\(\s*[\"']([a-z0-9_]+)[\"']")


def text_of(node) -> str:
    """Android's escaping undone: \\' \\" \\n and the entities the parser already handled."""
    t = "".join(node.itertext())
    t = t.replace("\\'", "'").replace('\\"', '"').replace("\\n", "\n").replace("\\@", "@")
    return t


def read_dir(res_dir, locale):
    """name -> text and name -> {quantity: text} for one locale's files."""
    folder = "values" if locale == "en" else f"values-{locale}"
    strings, plurals = {}, {}
    for path in sorted(glob.glob(os.path.join(res_dir, folder, "strings*.xml"))):
        root = ET.parse(path).getroot()
        for s in root.iter("string"):
            if s.get("name"):
                strings[s.get("name")] = text_of(s)
        for p in root.iter("plurals"):
            plurals[p.get("name")] = {i.get("quantity"): text_of(i) for i in p.iter("item")}
    return strings, plurals


def locales():
    found = {"en"}
    for res in (ANDROID_RES, DESK_RES):
        for d in glob.glob(os.path.join(res, "values-*")):
            loc = os.path.basename(d).split("values-")[1]
            if os.path.isdir(d) and not re.fullmatch(r"v\d+", loc):
                found.add(loc)
    return sorted(found)


def used_keys():
    keys = set()
    for path in glob.glob(os.path.join(SRC, "**", "*.svelte"), recursive=True) + glob.glob(os.path.join(SRC, "**", "*.ts"), recursive=True):
        # The runtime's own comments explain t("key"); they use no string.
        if os.path.basename(path) == "i18n.svelte.ts":
            continue
        with open(path, encoding="utf-8") as f:
            keys.update(USED.findall(f.read()))
    return keys


def build(loc, keys):
    strings, plurals = read_dir(ANDROID_RES, loc)
    ds, dp = read_dir(DESK_RES, loc)
    strings.update(ds)
    plurals.update(dp)
    out = {k: strings[k] for k in sorted(keys) if k in strings}
    pl = {k: plurals[k] for k in sorted(keys) if k in plurals}
    if pl:
        out["__plurals__"] = pl
    return out


def main() -> int:
    check = "--check" in sys.argv
    keys = used_keys()
    en = build("en", keys)
    missing = sorted(k for k in keys if k not in en and k not in en.get("__plurals__", {}))
    if missing:
        print("keys the pages use that no English file defines:", ", ".join(missing))
        return 1
    problems = 0
    os.makedirs(OUT, exist_ok=True)
    for loc in locales():
        d = build(loc, keys)
        absent = [k for k in keys if k not in d and k not in d.get("__plurals__", {})]
        if absent and loc != "en":
            # English fills the gap at runtime; this is the list to translate.
            print(f"  {loc}: {len(absent)} string(s) fall back to English" + (": " + ", ".join(sorted(absent)[:5]) + ("…" if len(absent) > 5 else "") if len(absent) <= 40 else ""))
        text = json.dumps(d, ensure_ascii=False, indent=1, sort_keys=True) + "\n"
        path = os.path.join(OUT, f"{loc}.json")
        if check:
            try:
                with open(path, encoding="utf-8") as f:
                    if f.read() != text:
                        print(f"  ! {loc}.json is stale — run strings_from_android.py")
                        problems += 1
            except FileNotFoundError:
                print(f"  ! {loc}.json is missing — run strings_from_android.py")
                problems += 1
        else:
            with open(path, "w", encoding="utf-8") as f:
                f.write(text)
    print(f"{len(keys)} key(s) across {len(locales())} locale(s)" + (" — up to date" if check and not problems else ""))
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
