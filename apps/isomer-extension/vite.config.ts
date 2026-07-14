import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import wasm from "vite-plugin-wasm";
import topLevelAwait from "vite-plugin-top-level-await";
import { resolve } from "path";

export default defineConfig({
  plugins: [react(), tailwindcss(), wasm(), topLevelAwait()],
  build: {
    outDir: "dist",
    emptyDirBeforeWrite: true,
    rollupOptions: {
      input: {
        popup: resolve(__dirname, "popup.html"),
        background: resolve(__dirname, "src/background/index.ts"),
        content: resolve(__dirname, "src/content/index.ts"),
        inpage: resolve(__dirname, "src/provider/inpage.ts"),
      },
      output: {
        entryFileNames: (chunkInfo) => {
          if (chunkInfo.name === "popup") {
            return "assets/[name]-[hash].js";
          }
          return "[name].js";
        },
        chunkFileNames: "assets/[name]-[hash].js",
        assetFileNames: "assets/[name]-[hash].[ext]",
      },
    },
  },
  resolve: {
    alias: {
      "@": resolve(__dirname, "src"),
    },
  },
});
