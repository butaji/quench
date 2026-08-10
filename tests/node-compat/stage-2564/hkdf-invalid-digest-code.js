const assert = require("assert");
const { hkdf, hkdfSync } = require("crypto");

for (
  const fn of [
    () => hkdfSync("unknown", "a", "", "", 10),
    () => hkdf("unknown", "a", "", "", 10, () => {}),
  ]
) {
  assert.throws(fn, (error) => {
    assert.strictEqual(error.code, "ERR_CRYPTO_INVALID_DIGEST");
    return true;
  });
}
console.log("HKDF invalid digest code passed");
