const assert = require("node:assert");
const crypto = require("node:crypto");

for (const input of [[0x1, 0x2], () => {}, /abc/, {}]) {
  assert.throws(() => crypto.createDiffieHellman(input), {
    code: "ERR_INVALID_ARG_TYPE",
    name: "TypeError",
  });
}
console.log("crypto DH input types passed");
