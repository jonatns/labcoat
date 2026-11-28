// Serves the wallet check page and forwards the same-origin path /rpc to the
// endpoint in LABCOAT_RPC_TARGET, as an app's own server does: Subfrost's
// hosted endpoints send no CORS header, and an API key stays out of the page.
import { defineConfig } from "vite"

const target = new URL(process.env["LABCOAT_RPC_TARGET"] ?? "http://127.0.0.1:18888")
const key = process.env["SUBFROST_API_KEY"]

export default defineConfig({
  esbuild: { jsx: "automatic" },
  server: {
    host: "localhost",
    port: Number(process.env["WALLET_CHECK_PORT"] ?? 5199),
    strictPort: true,
    proxy: {
      "/rpc": {
        target: target.origin,
        changeOrigin: true,
        rewrite: () => target.pathname,
        ...(key === undefined || key === "" ? {} : { headers: { "x-subfrost-api-key": key } }),
      },
    },
  },
})
