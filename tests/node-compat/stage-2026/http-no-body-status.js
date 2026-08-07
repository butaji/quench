const assert = require("assert");
const { ServerResponse } = require("http");

for (const statusCode of [204, 304]) {
  const response = new ServerResponse({ method: "GET" });
  response.writeHead(statusCode, { "Transfer-Encoding": "chunked" });
  response.end("ignored");
  assert.strictEqual(response.headers.connection, "close");
  assert.strictEqual(response.headers["transfer-encoding"], undefined);
  assert.strictEqual(response.headers["content-length"], undefined);
}

console.log("http no-body status passed");
