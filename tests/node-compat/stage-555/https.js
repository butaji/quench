"use strict";

const assert = require("assert");
const https = require("https");

for (const method of ["request", "get", "createServer"]) {
  assert.strictEqual(typeof https[method], "function");
  assert.throws(() => https[method](), { code: "ERR_TLS_NOT_SUPPORTED" });
}
assert.ok(https.globalAgent);

console.log("https boundary passed");
