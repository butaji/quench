const assert = require("node:assert");
const { internalBinding } = require("internal/test/binding");

const { validateRmOptionsSync } = require("internal/fs/utils");
assert.deepStrictEqual(validateRmOptionsSync("file"), {
  retryDelay: 100,
  maxRetries: 0,
  recursive: false,
  force: false,
});
assert.throws(() => validateRmOptionsSync("file", { retryDelay: -1 }), {
  code: "ERR_OUT_OF_RANGE",
});
assert.throws(() => validateRmOptionsSync("file", { recursive: undefined }), {
  code: "ERR_INVALID_ARG_TYPE",
});
assert.strictEqual(typeof internalBinding, "function");

console.log("rm option validation passed");
