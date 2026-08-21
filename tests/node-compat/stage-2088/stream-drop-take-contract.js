const assert = require("assert");
const { Readable } = require("stream");

(async () => {
  assert.deepStrictEqual(await Readable.from([1, 2, 3]).drop(2).toArray(), [3]);
  assert.deepStrictEqual(await Readable.from([1, 2, 3]).take(1).toArray(), [1]);
  assert.deepStrictEqual(await Readable.from([1, 2]).take("1").toArray(), [1]);
  assert.deepStrictEqual(await Readable.from([1, 2]).take(true).toArray(), [1]);
  assert.deepStrictEqual(await Readable.from([1, 2]).take(0).toArray(), []);
  assert.throws(() => Readable.from([]).take(-1), /ERR_OUT_OF_RANGE/);
})();
