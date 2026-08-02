const assert = require("assert");
const util = require("util");

assert.strictEqual(util.types.isNativeError(new Error()), true);
assert.strictEqual(
  util.types.isNativeError({ __proto__: Error.prototype }),
  false,
);
assert.throws(() => util.stripVTControlCharacters({}), {
  code: "ERR_INVALID_ARG_TYPE",
  name: "TypeError",
});
