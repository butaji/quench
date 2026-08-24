const assert = require("assert");

let warnings = 0;
process.on("warning", (warning) => {
  warnings++;
  assert.strictEqual(warning.name, "DeprecationWarning");
  assert.strictEqual(warning.code, "DEP0005");
});

Error.prepareStackTrace = () => new Buffer(4);
assert.ok(new Error().stack instanceof Buffer);
assert.ok(new Error().stack instanceof Buffer);
queueMicrotask(() => assert.strictEqual(warnings, 1));
