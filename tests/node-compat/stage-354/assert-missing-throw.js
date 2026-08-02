const assert = require("assert");
assert.throws(() => assert.throws(() => {}), {
  code: "ERR_ASSERTION",
  operator: "throws",
  message: "Missing expected exception.",
});
