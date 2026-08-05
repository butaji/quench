const assert = require("node:assert");
const crypto = require("node:crypto");

const key = crypto.createPublicKey("AAAAAAAA");
const exported = key.export({ type: "spki", format: "pem" });
assert.strictEqual(exported.source, "AAAAAAAA");
console.log("crypto exported zero-peer marker passed");
