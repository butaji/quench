const assert = require("assert");
const a = new Float32Array([0]);
const b = new Float32Array([-0]);
console.log(Object.is(a[0], -0), Object.is(b[0], -0));
assert.strictEqual(Object.is(b[0], -0), true);
assert.throws(() => assert.partialDeepStrictEqual(a, b), assert.AssertionError);
