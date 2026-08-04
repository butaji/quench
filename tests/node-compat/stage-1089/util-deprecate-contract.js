const assert = require("assert");
const util = require("util");
const internalUtil = require("internal/util");

function original(first, second) {
  return first + second;
}

const deprecated = util.deprecate(original, "stage deprecation");
assert.strictEqual(deprecated.length, original.length);
assert.strictEqual(deprecated.prototype, original.prototype);
assert.strictEqual(Object.getPrototypeOf(deprecated), original);
assert.strictEqual(deprecated(1, 2), 3);
assert.strictEqual(internalUtil.pendingDeprecate(original).length, 2);
let warningCount = 0;
process.on("warning", () => {
  warningCount += 1;
});
setImmediate(() => assert.strictEqual(warningCount, 1));
