"use strict";

const assert = require("assert");
const crypto = require("node:crypto");

const input = Uint8Array.from([113, 117, 101, 110, 99, 104]);
crypto.webcrypto.subtle.digest("SHA-256", input).then((digest) => {
  assert.ok(digest instanceof Uint8Array);
  assert.strictEqual(digest.length, 32);
});

console.log("crypto webcrypto digest passed");
