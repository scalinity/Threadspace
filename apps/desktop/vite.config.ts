import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Dev server on the exact origin the native shell allows (SPEC §18.6).
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    host: "localhost",
    port: 1420,
    strictPort: true,
  },
  build: {
    target: "es2024",
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: false,
    // three/webgpu is a single large module; this is the bundled local scene.
    chunkSizeWarningLimit: 2048,
  },
});
