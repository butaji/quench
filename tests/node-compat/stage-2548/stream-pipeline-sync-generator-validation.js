const assert = require("assert");
const { PassThrough, pipeline } = require("stream");

const source = new PassThrough();
assert.throws(
  () => pipeline(source, function* intermediate() {}, () => {}),
  { code: "ERR_INVALID_RETURN_VALUE" },
);
assert.strictEqual(source.destroyed, false);
console.log("stream pipeline sync generator validation passed");
