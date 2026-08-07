const assert = require("node:assert");
const tls = require("node:tls");

assert(tls.getCiphers().includes("aes256-sha"));
assert(tls.getCiphers().includes("tls_aes_128_ccm_8_sha256"));
console.log("TLS cipher list passed");
