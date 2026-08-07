const assert = require("node:assert");
const tls = require("node:tls");

assert.throws(() => tls.createSecureContext({ pfx: "sample" }), {
  message: "not enough data",
});
console.log("pfx input validation passed");
