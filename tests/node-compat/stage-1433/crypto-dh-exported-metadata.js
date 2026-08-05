const assert = require("node:assert");
const crypto = require("node:crypto");

const pair = crypto.generateKeyPairSync("ec", { namedCurve: "P-256" });
const exported = pair.publicKey.export({ type: "spki", format: "pem" });
assert.strictEqual(exported.dhParams.namedCurve, "P-256");
console.log("crypto DH exported metadata passed");
