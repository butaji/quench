const assert = require("assert");
const { Readable } = require("stream");

(async () => {
  const signals = [];
  assert.deepStrictEqual(
    await Readable.from([1, 2, 3, 4])
      .map(
        async (value, { signal }) => {
          signals.push(signal instanceof AbortSignal);
          await Promise.resolve();
          return value * 2;
        },
        { concurrency: 2 },
      )
      .toArray(),
    [2, 4, 6, 8],
  );
  assert.deepStrictEqual(signals, [true, true, true, true]);

  assert.deepStrictEqual(
    await Readable.from([1, 2, 3, 4])
      .filter(
        async (value, { signal }) => {
          assert.strictEqual(signal instanceof AbortSignal, true);
          return value % 2 === 0;
        },
        { concurrency: 2 },
      )
      .toArray(),
    [2, 4],
  );

  const visited = [];
  await Readable.from([1, 2, 3]).forEach(
    async (value, { signal }) => {
      visited.push([value, signal instanceof AbortSignal]);
      await Promise.resolve();
    },
    { concurrency: 2 },
  );
  assert.deepStrictEqual(visited, [
    [1, true],
    [2, true],
    [3, true],
  ]);

  const controller = new AbortController();
  let calls = 0;
  const pending = Readable.from([1, 2, 3, 4]).forEach(
    async (_value, { signal }) => {
      calls++;
      await new Promise((resolve) =>
        signal.addEventListener("abort", resolve, { once: true })
      );
    },
    { signal: controller.signal, concurrency: 2, highWaterMark: 0 },
  );
  await new Promise((resolve) => setImmediate(resolve));
  assert.strictEqual(calls, 2);
  controller.abort();
  await assert.rejects(pending, { name: "AbortError" });

  const infinite = Readable.from(
    (async function* () {
      while (true) yield 1;
    })(),
  ).map((value) => value + 1);
  const iterator = infinite[Symbol.asyncIterator]();
  assert.deepStrictEqual(await iterator.next(), { value: 2, done: false });
  await iterator.return();
  assert.strictEqual(infinite.destroyed, true);
})();
