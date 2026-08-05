const assert = require("node:assert");
const crypto = require("node:crypto");

crypto.diffieHellman({ privateKey: {}, publicKey: {} }, (error) => {
  assert.strictEqual(error.code, "ERR_INVALID_ARG_TYPE");
  console.log("crypto stateless DH callback passed");
});
