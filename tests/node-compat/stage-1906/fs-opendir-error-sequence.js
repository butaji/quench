const assert = require("assert");
const fs = require("fs");

(async () => {
  assert.throws(() => fs.opendirSync(__filename), /ENOTDIR/);
  assert.throws(
    () => fs.opendir(__filename),
    /TypeError \[ERR_INVALID_ARG_TYPE\]: The "callback" argument must be of type function/,
  );
  const error = await new Promise((resolve) => {
    fs.opendir(__filename, (value) => resolve(value));
  });
  assert.strictEqual(error.code, "ENOTDIR");
})();
