const assert = require("assert");
const tls = require("tls");

assert.strictEqual(typeof tls.getCertificateCompressionAlgorithms, "function");
assert.deepStrictEqual(tls.getCertificateCompressionAlgorithms(), []);
assert.strictEqual(tls.DEFAULT_MIN_VERSION, "TLSv1.2");
assert.strictEqual(tls.DEFAULT_MAX_VERSION, "TLSv1.3");
console.log("tls capability surface passed");
