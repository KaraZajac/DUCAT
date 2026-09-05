import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Tauri serves the built `dist/` in a release and proxies `devUrl` in
// development; the fixed port is what tauri.conf.json points at.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: ["es2021", "chrome100", "safari14"],
    minify: "esbuild",
    sourcemap: false,
  },
});
