#!/usr/bin/env python3
"""One-time: split index.html into template.html + locales/en.json.

Run locally (needs beautifulsoup4); the per-deploy build is build.mjs, which is
zero-dependency Node so the server needs nothing installed.

THE IMPORTANT RULE: a translatable unit is a whole block element WITH its inline
markup inside it, never a bare text node. Splitting

    <p>Contacts and receipts are <b>DHT records</b>, sealed end to end.</p>

into three fragments would make it untranslatable — German and Japanese put those
pieces in a different order, and a translator handed "are" and ", sealed end to
end." separately cannot do anything sensible with them. So the string is the
element's whole innerHTML and the translator moves the <b> where their language
needs it.

Keys are <section>.<NN> in document order. build.mjs --check reports any locale
that has drifted from the template.
"""
import json
import os
import re
import sys

from bs4 import BeautifulSoup, NavigableString, Tag

HERE = os.path.dirname(os.path.abspath(__file__))
DOCS = os.path.dirname(HERE)

# tags allowed to live INSIDE a translatable string
INLINE = {"a", "b", "strong", "i", "em", "code", "span", "br", "small",
          "sup", "sub", "abbr", "kbd", "u", "mark", "time"}
# elements whose innerHTML becomes one translatable unit
BLOCK = {"p", "h1", "h2", "h3", "h4", "h5", "h6", "li", "dt", "dd", "td", "th",
         "figcaption", "button", "summary", "caption", "legend", "label", "title"}
# attributes worth translating
ATTRS = ("alt", "title", "aria-label", "placeholder")

SKIP_ANCESTORS = {"script", "style", "svg", "code", "pre"}
# prose that lives inside a skipped ancestor but should still be translated
FORCE_CLASSES = {"rt-desc"}
# never translate: brand names and literal filesystem paths
LITERAL_CLASSES = {"rt-dir", "rt-star", "nav-brand", "footer-brand"}


def is_translatable_block(el):
    """True when el holds text and only inline children."""
    if el.name not in BLOCK:
        return False
    if not el.get_text(strip=True):
        return False
    for c in el.children:
        if isinstance(c, NavigableString):
            continue
        if isinstance(c, Tag) and c.name in INLINE:
            continue
        return False
    return True


def section_of(el):
    for p in el.parents:
        if isinstance(p, Tag) and p.get("id"):
            return p["id"]
        if isinstance(p, Tag) and p.name in ("header", "footer", "nav"):
            return p.name
    return "page"


def main():
    src = os.path.join(DOCS, "index.html")
    html = open(src).read()
    soup = BeautifulSoup(html, "html.parser")

    strings = {}
    counters = {}

    def newkey(sec):
        counters[sec] = counters.get(sec, 0) + 1
        return f"{sec}.{counters[sec]:02d}"

    # 1. block elements
    for el in soup.find_all(list(BLOCK)):
        if any(p.name in SKIP_ANCESTORS for p in el.parents if isinstance(p, Tag)):
            continue
        if not is_translatable_block(el):
            continue
        inner = "".join(str(c) for c in el.children).strip()
        if not inner:
            continue
        # icon-only controls (the ☀/☾ theme toggle) carry no words to translate
        if not re.search(r"[A-Za-z]{2,}", el.get_text(" ", strip=True)):
            continue
        k = newkey(section_of(el))
        strings[k] = inner
        el.clear()
        el.append(NavigableString(f"{{{{{k}}}}}"))

    # 2. bare text in leaf divs/spans that hold real prose but are not BLOCK tags
    for el in soup.find_all(["div", "span"]):
        # find_all() snapshots the tree, so an element whose parent was already
        # replaced above is now detached — extracting it would mint an orphan key
        # that nothing renders and that every translator would waste time on.
        if el.parent is None:
            continue
        if any(p.name in SKIP_ANCESTORS for p in el.parents if isinstance(p, Tag)):
            continue
        if el.find(list(BLOCK)) or el.find("div"):
            continue
        txt = el.get_text(strip=True)
        if not txt or "{{" in str(el):
            continue
        if not re.search(r"[A-Za-z]{2,}", txt):
            continue
        ok = all(isinstance(c, NavigableString) or (isinstance(c, Tag) and c.name in INLINE)
                 for c in el.children)
        if not ok:
            continue
        inner = "".join(str(c) for c in el.children).strip()
        k = newkey(section_of(el))
        strings[k] = inner
        el.clear()
        el.append(NavigableString(f"{{{{{k}}}}}"))

    # 2b. leaf <a>/<span> prose no block swallowed: the header nav links, and the
    # descriptions in the repo tree. The tree is a <pre>, normally skipped as
    # preformatted, but its right-hand column is prose; its left-hand column is
    # directory paths, which must stay literal — "core/" is not a word.
    for el in soup.find_all(["a", "span"]):
        if el.parent is None:
            continue
        cls = set(el.get("class") or [])
        if cls & LITERAL_CLASSES:
            continue
        in_skip = any(p.name in SKIP_ANCESTORS for p in el.parents if isinstance(p, Tag))
        if in_skip and not (cls & FORCE_CLASSES):
            continue
        if "{{" in str(el):
            continue
        txt = el.get_text(strip=True)
        if not txt or not re.search(r"[A-Za-z]{2,}", txt):
            continue
        if not all(isinstance(c, NavigableString) or (isinstance(c, Tag) and c.name in INLINE)
                   for c in el.children):
            continue
        inner = "".join(str(c) for c in el.children).strip()
        k = newkey(section_of(el))
        strings[k] = inner
        el.clear()
        el.append(NavigableString(f"{{{{{k}}}}}"))

    # 3. translatable attributes
    for el in soup.find_all(True):
        if el.parent is None:
            continue
        for a in ATTRS:
            v = el.get(a)
            if not v or not isinstance(v, str) or not re.search(r"[A-Za-z]{2,}", v):
                continue
            if "{{" in v:
                continue
            k = newkey(section_of(el) + ".attr")
            strings[k] = v
            el[a] = f"{{{{{k}}}}}"

    # 4. meta description / og / twitter text
    for el in soup.find_all("meta"):
        prop = el.get("property", "") or el.get("name", "")
        if prop in ("description", "og:title", "og:description",
                    "twitter:title", "twitter:description", "og:image:alt"):
            v = el.get("content", "")
            if v and "{{" not in v:
                k = newkey("meta")
                strings[k] = v
                el["content"] = f"{{{{{k}}}}}"

    # the build fills this in with plain <a> links, one per locale
    out_html = str(soup)
    if "{{__langs__}}" not in out_html:
        out_html = out_html.replace('<div class="footer-bottom">',
                                    '<div class="footer-bottom">\n{{__langs__}}', 1)

    os.makedirs(os.path.join(DOCS, "locales"), exist_ok=True)
    with open(os.path.join(DOCS, "template.html"), "w") as fh:
        fh.write(out_html)
    with open(os.path.join(DOCS, "locales", "en.json"), "w") as fh:
        json.dump(strings, fh, ensure_ascii=False, indent=2)
        fh.write("\n")

    words = sum(len(re.sub(r"<[^>]+>", " ", v).split()) for v in strings.values())
    print(f"  {len(strings)} strings, {words} words -> locales/en.json")
    print(f"  template.html written")


if __name__ == "__main__":
    sys.exit(main())
