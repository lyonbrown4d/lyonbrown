import { randomBytes } from "node:crypto";
import { resolve } from "node:path";
import react from "@vitejs/plugin-react";
import { defineConfig } from "electron-vite";

const alias = {
  "@lyonbrown/agent-desktop/internal": resolve(import.meta.dirname, "src"),
};
export default defineConfig({
  main: {
    resolve: { alias },
    build: {
      outDir: "dist/main",
      rollupOptions: {
        output: { chunkFileNames: "[name]-[hash].js" },
        input: {
          index: resolve(import.meta.dirname, "src/main/index.ts"),
          "client-worker": resolve(import.meta.dirname, "src/main/client-worker.ts"),
        },
      },
    },
  },
  preload: {
    resolve: { alias },
    build: {
      outDir: "dist/preload",
      externalizeDeps: false,
      rollupOptions: { output: { format: "cjs", entryFileNames: "index.cjs" } },
    },
  },
  renderer: {
    resolve: { alias },
    html: { cspNonce: "__LYONBROWN_DEVELOPMENT_NONCE__" },
    plugins: [
      react(),
      {
        name: "desktop-development-csp",
        transformIndexHtml: {
          order: "post",
          handler(html, context) {
            if (!context.server) return html;
            const nonce = randomBytes(18).toString("base64");
            return html
              .replaceAll("__LYONBROWN_DEVELOPMENT_NONCE__", nonce)
              .replace("script-src 'self'", "script-src 'self' 'nonce-" + nonce + "'")
              .replace("style-src 'self'", "style-src 'self' 'nonce-" + nonce + "'");
          },
        },
      },
    ],
    build: { outDir: resolve(import.meta.dirname, "dist/renderer"), minify: "esbuild" },
  },
});
