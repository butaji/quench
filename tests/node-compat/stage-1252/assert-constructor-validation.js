const assert = require("node:assert");

let rejected = false;
try {
  assert.throws(() => {
    throw {};
  }, Array);
} catch (_) {
  rejected = true;
}
assert.strictEqual(rejected, true);
assert.throws(() =>
  assert.doesNotThrow(() => {
    throw new TypeError("x");
  }, TypeError)
);

console.log("assert constructor validation passed");
