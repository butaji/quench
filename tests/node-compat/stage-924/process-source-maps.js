const assert = require("assert");

for (const value of [undefined, null, 1, {}, () => {}]) {
  assert.throws(
    () => process.setSourceMapsEnabled(value),
    (error) => error.code === "ERR_INVALID_ARG_TYPE",
  );
}

assert.strictEqual(process.setSourceMapsEnabled(true), undefined);
assert.strictEqual(process.__sourceMapsEnabled, true);
assert.strictEqual(process.setSourceMapsEnabled(false), undefined);
assert.strictEqual(process.__sourceMapsEnabled, false);
