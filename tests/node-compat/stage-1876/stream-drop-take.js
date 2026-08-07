const assert = require("assert");
const { Readable } = require("stream");

(async () => {
  assert.deepStrictEqual(await Readable.from([1, 2, 3]).drop(2).toArray(), [3]);
  assert.deepStrictEqual(await Readable.from([1, 2, 3]).take(1).toArray(), [1]);
  assert.deepStrictEqual(
    await Readable.from([1, 2, 3]).drop(1).take(1).toArray(),
    [2],
  );
})();
