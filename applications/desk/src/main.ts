import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";
import { applyLanguage } from "./lib/i18n.svelte";

// The appearance chosen on the Me page, applied before the first paint.
try {
  const theme = localStorage.getItem("ducat.theme");
  if (theme === "light" || theme === "dark") document.documentElement.dataset.theme = theme;
} catch {}

// The language too: the dictionaries are small, and a page must never
// flash English before its own words.
await applyLanguage();

const app = mount(App, { target: document.getElementById("app")! });

export default app;
