const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(
  () => fs.opendir(__filename),
  /TypeError \[ERR_INVALID_ARG_TYPE\]: The "callback" argument must be of type function/,
);
assert.throws(() => fs.opendirSync(__filename), /ENOTDIR/);
assert.throws(() => fs.opendir(false, () => {}), {
  code: "ERR_INVALID_ARG_TYPE",
});
assert.throws(() => fs.opendirSync(false), { code: "ERR_INVALID_ARG_TYPE" });
const directory = fs.opendirSync(".");
assert.throws(() => directory.constructor.prototype.path, {
  code: "ERR_INVALID_THIS",
});
directory.closeSync();
assert.throws(() => directory.closeSync(), { code: "ERR_DIR_CLOSED" });

console.log("opendir callback error passed");
