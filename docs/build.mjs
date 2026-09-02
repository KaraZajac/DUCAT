// Build ducatproject.org in every language the DUCAT app ships.
//
// Renders template.html once per locale in locales/*.json into dist/, and
// copies the shared assets. Zero dependencies on purpose: this runs on the
// server during site-deploy, which has plain Node and nothing installed.
//
// English is dist/index.html; every other language is dist/<code>/index.html,
// so assets resolve with a "../" prefix from exactly one level down. Nothing is
// duplicated per language — the images alone would be tens of megabytes.
//
//   node build.mjs           build dist/
//   node build.mjs --check   report locale drift, build nothing
//
// Translations are expected to match the terminology the Android app already
// uses (Bond → Kaution/Fianza/保証金, escrow → Treuhand/séquestre/托管, …).
// The site and the app are one product; they must not use different words.

import { readFileSync, writeFileSync, mkdirSync, readdirSync, rmSync, cpSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const DIST = join(HERE, "dist");

// Language display names, in the language itself — a switcher that names
// languages in English is useless to the person who needs it.
const LANGS = {
  en: { name: "English",    dir: "ltr" },
  ar: { name: "العربية",     dir: "rtl" },
  de: { name: "Deutsch",    dir: "ltr" },
  es: { name: "Español",    dir: "ltr" },
  fa: { name: "فارسی",       dir: "rtl" },
  fr: { name: "Français",   dir: "ltr" },
  hi: { name: "हिन्दी",        dir: "ltr" },
  id: { name: "Indonesia",  dir: "ltr" },
  it: { name: "Italiano",   dir: "ltr" },
  ja: { name: "日本語",       dir: "ltr" },
  ko: { name: "한국어",       dir: "ltr" },
  nl: { name: "Nederlands", dir: "ltr" },
  pl: { name: "Polski",     dir: "ltr" },
  pt: { name: "Português",  dir: "ltr" },
  ru: { name: "Русский",    dir: "ltr" },
  th: { name: "ไทย",         dir: "ltr" },
  tr: { name: "Türkçe",     dir: "ltr" },
  uk: { name: "Українська", dir: "ltr" },
  vi: { name: "Tiếng Việt",  dir: "ltr" },
  zh: { name: "中文",         dir: "ltr" },
};

const SITE = "https://ducatproject.org";
const ASSETS = ["style.css", "app.js", "mascot.png", "og.png", "fonts"];

const template = readFileSync(join(HERE, "template.html"), "utf8");
const localeDir = join(HERE, "locales");
const available = readdirSync(localeDir).filter(f => f.endsWith(".json")).map(f => f.slice(0, -5));
const en = JSON.parse(readFileSync(join(localeDir, "en.json"), "utf8"));
const keys = Object.keys(en);

// ---- drift + markup check ------------------------------------------------
// Across nineteen languages the likeliest breakage is not a bad sentence, it is
// a dropped </strong> or a mangled href — damage a reader sees as a broken page
// rather than an awkward phrase. So every string is checked to carry the same
// tags and the same link targets as its English source.
const tagsOf = s => (String(s).match(/<\/?([a-z0-9]+)/gi) || []).map(t => t.toLowerCase()).sort().join(",");
const hrefsOf = s => (String(s).match(/(?:href|src)="([^"]*)"/gi) || []).sort().join(",");
const varsOf = s => (String(s).match(/data-target="[^"]*"/gi) || []).sort().join(",");

let problems = 0;
for (const code of available) {
  const s = JSON.parse(readFileSync(join(localeDir, `${code}.json`), "utf8"));
  const missing = keys.filter(k => !(k in s) || !String(s[k]).trim());
  const extra = Object.keys(s).filter(k => !keys.includes(k));
  const broken = [];
  for (const k of keys) {
    if (!(k in s)) continue;
    if (tagsOf(s[k]) !== tagsOf(en[k])) broken.push(`${k} (tags)`);
    else if (hrefsOf(s[k]) !== hrefsOf(en[k])) broken.push(`${k} (links)`);
    else if (varsOf(s[k]) !== varsOf(en[k])) broken.push(`${k} (counters)`);
  }
  if (missing.length || extra.length || broken.length) {
    problems++;
    console.log(`  ${code}: ${missing.length} missing, ${extra.length} stale, ${broken.length} markup`);
    if (missing.length) console.log(`      missing: ${missing.slice(0, 8).join(", ")}${missing.length > 8 ? " …" : ""}`);
    if (extra.length) console.log(`      stale:   ${extra.slice(0, 8).join(", ")}${extra.length > 8 ? " …" : ""}`);
    if (broken.length) console.log(`      markup:  ${broken.slice(0, 8).join(", ")}${broken.length > 8 ? " …" : ""}`);
  }
  if (!LANGS[code]) { console.log(`  ${code}: no entry in LANGS`); problems++; }
}
if (process.argv.includes("--check")) {
  console.log(problems ? `  ${problems} locale(s) need attention` : `  all ${available.length} locales complete (${keys.length} strings)`);
  process.exit(problems ? 1 : 0);
}
if (problems) console.log(`  (building anyway; untranslated keys fall back to English)`);

// ---- render --------------------------------------------------------------
const order = available.slice().sort((a, b) => (a === "en" ? -1 : b === "en" ? 1 : a.localeCompare(b)));

function switcher(current, prefix) {
  // Plain links, no JS: Tor Browser at its Safest level runs no script, and this
  // is exactly the audience that needs a language other than English.
  const items = order.map(code => {
    const href = code === "en" ? (prefix || "./") : `${prefix}${code}/`;
    const cur = code === current ? ' aria-current="true"' : "";
    return `<li><a href="${href}" hreflang="${code}" lang="${code}"${cur}>${LANGS[code].name}</a></li>`;
  });
  return `<nav class="langs" aria-label="Language"><ul>${items.join("")}</ul></nav>`;
}

function alternates(prefix) {
  const links = order.map(code => {
    const href = code === "en" ? `${SITE}/` : `${SITE}/${code}/`;
    return `<link rel="alternate" hreflang="${code}" href="${href}">`;
  });
  links.push(`<link rel="alternate" hreflang="x-default" href="${SITE}/">`);
  return links.join("\n");
}

rmSync(DIST, { recursive: true, force: true });
mkdirSync(DIST, { recursive: true });

let built = 0;
for (const code of order) {
  const s = { ...en, ...JSON.parse(readFileSync(join(localeDir, `${code}.json`), "utf8")) };
  const meta = LANGS[code];
  const prefix = code === "en" ? "" : "../";

  let out = template.replace(/\{\{([a-zA-Z0-9_.]+)\}\}/g, (m, k) =>
    k in s ? String(s[k]) : (k in en ? String(en[k]) : m));

  // language + direction
  out = out.replace(/<html[^>]*>/, `<html lang="${code}" dir="${meta.dir}">`);

  // assets live at the root; localised pages sit one level down
  if (prefix) {
    out = out.replace(/(\s(?:href|src)=")(?!https?:|\/\/|\/|#|data:|mailto:)/g, `$1${prefix}`);
  }

  // canonical + alternates
  const canonical = code === "en" ? `${SITE}/` : `${SITE}/${code}/`;
  // attribute order is whatever the template happens to use, so do not assume
  // rel comes first — matching on that silently left every translation
  // declaring itself a duplicate of the English page.
  out = out.replace(/<link[^>]*rel="canonical"[^>]*>/, `<link rel="canonical" href="${canonical}">`);
  // og:url shares the problem: left alone, every translation would advertise the
  // English page as its own address when shared.
  out = out.replace(/<meta[^>]*property="og:url"[^>]*>/, `<meta property="og:url" content="${canonical}">`);
  out = out.replace("</head>", `${alternates(prefix)}\n</head>`);

  // switcher goes wherever the template asks for it
  out = out.replace(/\{\{__langs__\}\}/g, switcher(code, prefix));

  const dir = code === "en" ? DIST : join(DIST, code);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "index.html"), out);
  built++;
}

// ---- assets (one copy, at the root) --------------------------------------
for (const a of ASSETS) {
  const src = join(HERE, a);
  if (existsSync(src)) cpSync(src, join(DIST, a), { recursive: true });
}
// the screenshot gallery lives at the repo root because the README cites it there
const images = join(HERE, "..", "images");
if (existsSync(images)) {
  cpSync(images, join(DIST, "images"), { recursive: true });
  rmSync(join(DIST, "images", "README.md"), { force: true });
}

console.log(`  built ${built} language(s) — ${keys.length} strings each`);
