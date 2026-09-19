import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

/**
 * The dev server proxies the API to `jev-desk-server`, so the browser talks to
 * one origin in development and in production alike and the client never needs
 * a base URL.
 */
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      "/api": {
        target: process.env.JEV_API ?? "http://127.0.0.1:8787",
        changeOrigin: true,
      },
    },
  },
  build: {
    outDir: "dist",
    sourcemap: true,
  },
});
