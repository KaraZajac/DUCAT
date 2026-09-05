import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";

// The appearance chosen on the Me page, applied before the first paint.
try {
  const theme = localStorage.getItem("ducat.theme");
  if (theme === "light" || theme === "dark") document.documentElement.dataset.theme = theme;
} catch {}

const app = mount(App, { target: document.getElementById("app")! });

export default app;
