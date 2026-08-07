const assert = require("assert");
const { sleep } = require("internal/util");

for (const value of [undefined, null, "", {}, true, false]) {
  assert.throws(() => sleep(value), /must be of type number/);
}
for (const value of [-1, 3.14, NaN, 4294967296]) {
  assert.throws(() => sleep(value), /out of range/);
}
