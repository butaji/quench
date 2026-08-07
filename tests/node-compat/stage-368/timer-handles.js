const assert = require("assert");
let value;
setTimeout(
  (first, second) => {
    value = first + second;
    assert.strictEqual(value, 5);
  },
  1,
  2,
  3,
);
const cancelled = setImmediate(() => {
  throw new Error("cancelled immediate ran");
});
clearImmediate(cancelled);
