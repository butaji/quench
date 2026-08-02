const assert = require("assert");
assert.throws(() => {
  throw new TypeError({});
}, /\[object Object\]/);
assert.throws(
  () => {
    throw new TypeError({});
  },
  (error) => error instanceof TypeError,
);
