// Extracts the wallet integration examples so the check page runs the
// same code shown in the documentation.
import { mkdirSync, readFileSync, writeFileSync } from "node:fs"

const [page, outDir] = process.argv.slice(2)
const blocks = [...readFileSync(page, "utf8").matchAll(/```ts\n([\s\S]*?)```/g)].map((match) => match[1])
const recipes = {
  "lasereyes.ts": "export function useLabcoatWallet(",
  "sats-connect.ts": "export const satsConnectWallet",
  "subfrost-remote.ts": "export function subfrostRemoteWallet(",
}
mkdirSync(outDir, { recursive: true })
for (const [file, marker] of Object.entries(recipes)) {
  const found = blocks.filter((block) => block.includes(marker))
  if (found.length !== 1) throw new Error(`expected one recipe with "${marker}" in ${page}, found ${found.length}`)
  writeFileSync(`${outDir}/${file}`, found[0].replaceAll("./generated/web/", "../generated/"))
}
