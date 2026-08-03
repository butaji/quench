"use strict";

const assert = require("assert");
const http2 = require("http2");

for (const method of ["createServer", "connect"]) {
  assert.strictEqual(typeof http2[method], "function");
  assert.throws(() => http2[method](), { code: "ERR_HTTP2_NOT_SUPPORTED" });
}
assert.strictEqual(http2.constants.NGHTTP2_SESSION_SERVER, 1);

console.log("http2 boundary passed");
