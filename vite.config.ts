import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// The dev page, on this Mac only: it proxies the whole API (http.rs), so it
// never listens on the network. To open it from another Mac on the tailnet,
// `tailscale serve 1420` here.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
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
