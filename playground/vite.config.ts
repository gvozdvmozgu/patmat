import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const projectRoot = dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  base: "/patmat/",
  plugins: [react()],
  server: {
    fs: {
      allow: [projectRoot, resolve(projectRoot, "../playground-wasm/pkg")],
    },
  },
});
