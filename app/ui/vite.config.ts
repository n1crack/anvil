import { resolve } from "node:path";
import { defineConfig } from "vite-plus";

// Three windows, each its own page; Tauri serves `dist` (see app/tauri.conf.json).
export default defineConfig({
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "safari16",
    rollupOptions: {
      input: {
        main: resolve(import.meta.dirname, "index.html"),
        settings: resolve(import.meta.dirname, "settings.html"),
        processes: resolve(import.meta.dirname, "processes.html"),
      },
    },
  },
  test: { environment: "happy-dom", include: ["test/**/*.test.ts"] },
  // Only the code is formatted; the HTML and CSS keep their hand layout.
  fmt: { printWidth: 140, ignorePatterns: ["dist/**", "pnpm-lock.yaml", "*.html", "*.css"] },
  lint: {
    ignorePatterns: ["dist/**"],
    // Terminal output is made of control characters: matching them is the point.
    overrides: [{ files: ["src/term.ts"], rules: { "no-control-regex": "off" } }],
  },
});
