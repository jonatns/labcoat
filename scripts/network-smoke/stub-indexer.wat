;; A stand-in for alkanes.wasm, used only by the Labcoat Network smoke test.
;; It indexes nothing, so the test needs no release asset, while the chain,
;; mempool, validation, Esplora queries, and faucet underneath are real.
;; Labcoat Network loads WebAssembly text as well as binary modules.
(module
  (memory (export "memory") 1)
  (func (export "_start")))
