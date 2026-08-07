const assert = require("node:assert");
const tls = require("node:tls");

assert.throws(
  () => tls.createSecureContext({ crl: "not a CRL" }),
  (error) =>
    error instanceof Error &&
    error.message === "Failed to parse CRL" &&
    !("opensslErrorStack" in error),
);
console.log("TLS CRL validation passed");
