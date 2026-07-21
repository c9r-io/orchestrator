import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  // Tauri serves bundled assets from its own application origin. Relative
  // paths keep production builds loadable there instead of resolving assets
  // from the origin root (which renders a blank window on macOS WebView).
  base: "./",
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
});
