"use strict";

const assert = require("assert");
const processApi = require("process");

for (
  const name of [
    "cached_builtins",
    "debug",
    "inspector",
    "ipv6",
    "openssl_is_boringssl",
    "quic",
    "require_module",
    "tls",
    "tls_alpn",
    "tls_ocsp",
    "tls_sni",
    "uv",
  ]
) {
  assert.strictEqual(typeof processApi.features[name], "boolean");
}
assert.strictEqual(typeof processApi.features.typescript, "string");

console.log("process features passed");
