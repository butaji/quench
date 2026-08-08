const assert = require("assert");
const { Readable } = require("stream");

const fromAsync = (...args) =>
  Readable.from(...args).map(async (value) => value);
const naturals = () =>
  Readable.from(
    (async function* () {
      let value = 1;
      while (true) yield value++;
    })()
  );

(async () => {
  const cases = [
    ["sync drop", () => Readable.from([1, 2, 3]).drop(2).toArray(), [3]],
    ["sync take", () => Readable.from([1, 2, 3]).take(1).toArray(), [1]],
    [
      "sync chain",
      () => Readable.from([1, 2, 3]).drop(1).take(1).toArray(),
      [2]
    ],
    ["async drop", () => fromAsync([1, 2, 3]).drop(2).toArray(), [3]],
    ["async take", () => fromAsync([1, 2, 3]).take(1).toArray(), [1]],
    ["infinite take", () => naturals().take(1).toArray(), [1]],
    ["infinite chain", () => naturals().drop(1).take(1).toArray(), [2]]
  ];
  for (const [name, operation, expected] of cases) {
    const actual = await Promise.race([
      operation(),
      new Promise((_, reject) =>
        setTimeout(() => reject(new Error(`${name} timeout`)), 100)
      )
    ]);
    assert.deepStrictEqual(actual, expected, name);
    console.log(`passed ${name}`);
  }
  const controller = new AbortController();
  const aborted = Readable.from([1, 2, 3])
    .take(1, {
      signal: controller.signal
    })
    .toArray();
  controller.abort();
  await assert.rejects(aborted, { name: "AbortError" });
  const alreadyAborted = AbortSignal.abort();
  await assert.rejects(
    Readable.from([1, 2, 3]).take(1, { signal: alreadyAborted }).toArray(),
    { name: "AbortError" }
  );
  console.log("passed abort cases");
})();
