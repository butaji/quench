const assert = require("assert");
const path = require("path").win32;

for (const value of [true, 7, null, {}, undefined, [], NaN]) {
  assert.throws(() => path.join(value), { code: "ERR_INVALID_ARG_TYPE" });
  assert.throws(() => path.normalize(value), { code: "ERR_INVALID_ARG_TYPE" });
  assert.throws(() => path.isAbsolute(value), { code: "ERR_INVALID_ARG_TYPE" });
  assert.throws(() => path.basename(value), { code: "ERR_INVALID_ARG_TYPE" });
}
