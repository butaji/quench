const assert = require("assert");

for (const value of [null, undefined, 1, "callback", {}, []]) {
  assert.throws(
    () => process.nextTick(value),
    (error) => error.code === "ERR_INVALID_ARG_TYPE",
  );
}
