const assert = require("node:assert");
const crypto = require("node:crypto");

for (
  const update of [
    () => crypto.createHash("sha1").update("0", "hex"),
    () => crypto.createHmac("sha256", "secret").update("0", "hex"),
  ]
) {
  assert.throws(update, { code: "ERR_INVALID_ARG_VALUE", name: "TypeError" });
}
console.log("crypto hex encoding validation passed");
