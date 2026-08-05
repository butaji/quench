const assert = require("node:assert");
const tls = require("node:tls");

assert.deepStrictEqual(tls.getCiphers(), [
  "aes256-sha",
  "tls_aes_128_ccm_8_sha256"
]);
console.log("TLS cipher list passed");
