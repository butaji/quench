const assert = require("assert");
const { Readable } = require("stream");

async function collect(stream) {
  const values = [];
  for await (const value of stream) values.push(value);
  return values;
}

(async () => {
  assert.strictEqual(typeof Readable.prototype.map, "function");
  assert.deepStrictEqual(
    await collect(Readable.from([1, 2, 3]).map((value) => value * 2)),
    [2, 4, 6],
  );
  assert.deepStrictEqual(
    await collect(
      Readable.from([1, 2, 3, 4]).filter((value) => value % 2 === 0),
    ),
    [2, 4],
  );
  const seen = [];
  await Readable.from([1, 2]).forEach((value) => seen.push(value));
  assert.deepStrictEqual(seen, [1, 2]);
  console.log("stream combinators passed");
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
