"use strict";

const assert = require("assert");
const tls = require("tls");

assert.ok(tls.getCiphers().length > 0);
assert.strictEqual(tls.DEFAULT_MIN_VERSION, "TLSv1.2");
assert.strictEqual(tls.DEFAULT_MAX_VERSION, "TLSv1.3");
assert.deepStrictEqual(
  tls.createSecureContext({ minVersion: "TLSv1.3" }).context,
  {
    minVersion: "TLSv1.3",
  },
);
assert.throws(() => tls.connect(443, "example.com"), {
  code: "ERR_TLS_NOT_SUPPORTED",
});

console.log("tls surface passed");
