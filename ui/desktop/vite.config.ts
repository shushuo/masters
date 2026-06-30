import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri serves the built `dist/` in production and the Vite dev server in development.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    outDir: "dist",
    target: "es2021",
  },
});
