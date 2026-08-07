const assert = require("assert");
const { Readable } = require("stream");

const fromAsync = (...args) =>
  Readable.from(...args).map(async (value) => value);

(async () => {
  assert.deepStrictEqual(await fromAsync([1, 2, 3]).drop(2).toArray(), [3]);
  assert.deepStrictEqual(await fromAsync([1, 2, 3]).take(1).toArray(), [1]);
  assert.deepStrictEqual(
    await Readable.from([1, 2, 3]).drop(1).take(1).toArray(),
    [2],
  );
  assert.deepStrictEqual(await Readable.from([1, 2]).take("2").toArray(), [
    1,
    2,
  ]);
  assert.deepStrictEqual(await Readable.from([1, 2]).take(true).toArray(), [1]);
  assert.deepStrictEqual(await Readable.from([1, 2]).take("cat").toArray(), []);

  let pulled = false;
  let release;
  const gate = new Promise((resolve) => {
    release = resolve;
  });
  const stream = Readable.from((async function* () {
    yield 1;
    await gate;
    pulled = true;
    yield 2;
  })());
  assert.deepStrictEqual(await stream.take(1).toArray(), [1]);
  assert.strictEqual(pulled, false);
  release();

  const controller = new AbortController();
  const promise = Readable.from([1, 2, 3]).drop(1, {
    signal: controller.signal,
  }).toArray();
  controller.abort();
  await assert.rejects(promise, { name: "AbortError" });

  assert.throws(() => Readable.from([]).take(-1).toArray(), /ERR_OUT_OF_RANGE/);
  assert.throws(() => Readable.from([]).drop(1, 1), /ERR_INVALID_ARG_TYPE/);
})();
