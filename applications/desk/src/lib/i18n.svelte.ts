// The desk in the reader's language. The dictionaries under ./strings are
// generated from the phone's resources by scripts/strings_from_android.py,
// so a sentence the phone says is said here in the same nineteen languages;
// English is complete by construction and fills any gap.
//
// `t("key")` is a page's only door to a string; `%1$s` and friends are
// Android's positional placeholders and are filled in order. `tp("key", n)`
// picks the plural form the language uses for n, by Intl.PluralRules, the
// way Android does by CLDR.

type Dict = Record<string, string> & { __plurals__?: Record<string, Record<string, string>> };

const files = import.meta.glob<{ default: Dict }>("./strings/*.json");

export const LANGS: { code: string; name: string }[] = [
  { code: "en", name: "English" },
  { code: "ar", name: "العربية" },
  { code: "de", name: "Deutsch" },
  { code: "es", name: "Español" },
  { code: "fa", name: "فارسی" },
  { code: "fr", name: "Français" },
  { code: "hi", name: "हिन्दी" },
  { code: "id", name: "Bahasa Indonesia" },
  { code: "it", name: "Italiano" },
  { code: "ja", name: "日本語" },
  { code: "ko", name: "한국어" },
  { code: "nl", name: "Nederlands" },
  { code: "pl", name: "Polski" },
  { code: "pt", name: "Português" },
  { code: "ru", name: "Русский" },
  { code: "th", name: "ไทย" },
  { code: "tr", name: "Türkçe" },
  { code: "uk", name: "Українська" },
  { code: "vi", name: "Tiếng Việt" },
  { code: "zh", name: "中文" },
];

const RTL = new Set(["ar", "fa"]);

function systemLang(): string {
  const wanted = (navigator.languages?.length ? navigator.languages : [navigator.language || "en"]).map((l) => l.toLowerCase());
  for (const w of wanted) {
    const base = w.split("-")[0];
    if (LANGS.some((l) => l.code === base)) return base;
  }
  return "en";
}

function stored(): string | null {
  try {
    return localStorage.getItem("ducat.lang");
  } catch {
    return null;
  }
}

export const i18n = $state({
  /** "" means follow the system. */
  choice: stored() ?? "",
  lang: "en",
  dict: {} as Dict,
  en: {} as Dict,
  ready: false,
});

async function load(code: string): Promise<Dict> {
  const loader = files[`./strings/${code}.json`];
  if (!loader) return {};
  return (await loader()).default;
}

/** Applies the chosen (or system) language; safe to call again. */
export async function applyLanguage(choice?: string) {
  if (choice !== undefined) {
    i18n.choice = choice;
    try {
      if (choice) localStorage.setItem("ducat.lang", choice);
      else localStorage.removeItem("ducat.lang");
    } catch {}
  }
  const code = i18n.choice || systemLang();
  if (!Object.keys(i18n.en).length) i18n.en = await load("en");
  i18n.dict = code === "en" ? i18n.en : await load(code);
  i18n.lang = code;
  document.documentElement.lang = code;
  document.documentElement.dir = RTL.has(code) ? "rtl" : "ltr";
  i18n.ready = true;
}

function fill(text: string, args: (string | number)[]): string {
  if (!args.length) return text;
  let i = 0;
  return text.replace(/%(\d+)\$[sd]|%[sd]/g, (m, n) => {
    const idx = n ? Number(n) - 1 : i++;
    const v = args[idx];
    return v === undefined ? m : String(v);
  });
}

/** A string in the reader's language, English when the language lacks it, the key when nobody has it. */
export function t(key: string, ...args: (string | number)[]): string {
  const text = i18n.dict[key] ?? i18n.en[key] ?? key;
  return fill(text, args);
}

/** A plural in the reader's language; the count is also %1$d unless other args are given. */
export function tp(key: string, n: number, ...args: (string | number)[]): string {
  const forms = i18n.dict.__plurals__?.[key] ?? i18n.en.__plurals__?.[key];
  if (!forms) return t(key, n, ...args);
  let cat = "other";
  try {
    cat = new Intl.PluralRules(i18n.lang).select(n);
  } catch {}
  const text = forms[cat] ?? forms.other ?? Object.values(forms)[0] ?? key;
  return fill(text, args.length ? args : [n]);
}

/** The locale the numbers, times and money should follow. */
export function locale(): string {
  return i18n.lang === "en" ? navigator.language || "en" : i18n.lang;
}
