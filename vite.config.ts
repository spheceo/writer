import { hostname } from "node:os";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const name = hostname().split(".")[0].toLowerCase();

// The page is opened from other machines as http://kenobi:1420. The Rust API
// stays on localhost; this server proxies it.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "0.0.0.0",
    allowedHosts: [name, "kenobi", ".ts.net"],
    proxy: {
      "/api": {
        target: "http://127.0.0.1:1421",
        // The API answers only its own address (http.rs).
        changeOrigin: true,
        timeout: 0,
        proxyTimeout: 0,
      },
    },
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: "es2022",
    sourcemap: !!process.env.TAURI_DEBUG,
  },
});
