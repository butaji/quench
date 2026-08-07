const assert = require("assert");
const pairs = [
  [new Float32Array([0]), new Float32Array([-0])],
  [new Float64Array([0]), new Float64Array([-0])],
  [new Uint8Array(2), new Uint8Array(3)],
  [new Uint8Array([1, 2, 3]), new Uint8Array([4, 5, 6])],
  [new Uint16Array([2]), new Uint16Array([3])],
  [new ArrayBuffer(3), new SharedArrayBuffer(3)],
];
for (let index = 0; index < pairs.length; index++) {
  const [actual, expected] = pairs[index];
  let threw = false;
  try {
    assert.partialDeepStrictEqual(actual, expected);
  } catch (_) {
    threw = true;
  }
  console.log(index, threw);
  assert.strictEqual(threw, true);
}
