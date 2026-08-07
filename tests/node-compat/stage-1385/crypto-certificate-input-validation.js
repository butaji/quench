const assert = require("node:assert");
const crypto = require("node:crypto");

for (const value of [1, {}, [], Infinity, true, undefined, null]) {
  assert.throws(() => crypto.Certificate.verifySpkac(value), {
    code: "ERR_INVALID_ARG_TYPE",
  });
}
console.log("crypto certificate input validation passed");
