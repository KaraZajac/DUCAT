#!/usr/bin/env python3
"""Write one locale's strings_desk.xml from a JSON object on stdin.

    python3 write_desk_strings.py de < de.json

Plain keys map to strings; a key whose value is an object maps to a
plurals block, quantity by quantity. Apostrophes are escaped the way
Android wants them, and the key order follows the English file so diffs
read in the same order everywhere.
"""
import json
import os
import sys
import xml.etree.ElementTree as ET
from xml.sax.saxutils import escape

HERE = os.path.dirname(os.path.abspath(__file__))
STRINGS = os.path.join(os.path.dirname(HERE), "strings")


def main():
    loc = sys.argv[1]
    data = json.load(sys.stdin)
    root = ET.parse(os.path.join(STRINGS, "values", "strings_desk.xml")).getroot()
    order = []
    for el in root:
        if el.tag in ("string", "plurals"):
            order.append((el.tag, el.get("name")))
    lines = ['<?xml version="1.0" encoding="utf-8"?>', "<resources>"]
    missing = []
    for tag, name in order:
        if name not in data:
            missing.append(name)
            continue
        v = data[name]
        if tag == "string":
            lines.append(f'    <string name="{name}">{escape(str(v)).replace("\'", chr(92) + chr(39))}</string>')
        else:
            lines.append(f'    <plurals name="{name}">')
            for q, text in v.items():
                lines.append(f'        <item quantity="{q}">{escape(str(text)).replace("\'", chr(92) + chr(39))}</item>')
            lines.append("    </plurals>")
    lines.append("</resources>")
    extra = sorted(set(data) - {n for _, n in order})
    out_dir = os.path.join(STRINGS, f"values-{loc}")
    os.makedirs(out_dir, exist_ok=True)
    with open(os.path.join(out_dir, "strings_desk.xml"), "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")
    print(f"{loc}: {len(order) - len(missing)} written" + (f", missing {missing}" if missing else "") + (f", unknown {extra}" if extra else ""))


if __name__ == "__main__":
    main()
