const assert = require("assert");
const { createRequire } = require("module");

assert.throws(() => createRequire("https://example.com/app.js"), {
  code: "ERR_INVALID_ARG_VALUE",
});
assert.throws(() => createRequire("../app.js"), {
  code: "ERR_INVALID_ARG_VALUE",
});
assert.throws(() => createRequire({}), {
  code: "ERR_INVALID_ARG_VALUE",
  message: /must be a file URL object/,
});

assert.strictEqual(typeof createRequire("file:///tmp/app.js"), "function");
console.log("module createRequire validation passed");
