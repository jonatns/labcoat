#!/usr/bin/env node
// Stub of the unified JSON-RPC aggregator for devnet smoke tests.
// Answers anything on $PORT (default 18888) so health checks pass.
const http = require("http");
const port = parseInt(process.env.PORT || "18888", 10);
const host = process.env.HOST || "127.0.0.1";

http
  .createServer((req, res) => {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      res.writeHead(200, { "Content-Type": "application/json" });
      let id = null;
      try {
        id = JSON.parse(body || "{}").id ?? null;
      } catch {}
      res.end(JSON.stringify({ jsonrpc: "2.0", id, result: "stub-ok" }));
    });
  })
  .listen(port, host, () => console.log(`[stub-jsonrpc] listening on ${host}:${port}`));
