const assert = require("assert");
const { Readable } = require("stream");

const inputs = Array.from({ length: 8 }, (_unused, index) =>
  Array.from({ length: 20 }, (_item, value) => value + index)
);
const pending = inputs.map((input) =>
  Readable.from(input)
    .map(
      async (value) => {
        await Promise.resolve();
        return value * 2;
      },
      { concurrency: 6, highWaterMark: 5 }
    )
    .toArray()
);

assert.throws(() => Readable.from([1]).map(1), /ERR_INVALID_ARG_TYPE/);
assert.throws(
  () => Readable.from([1]).map((value) => value, { concurrency: "invalid" }),
  /ERR_OUT_OF_RANGE/
);
assert.throws(
  () => Readable.from([1]).map((value) => value, { concurrency: -1 }),
  /ERR_OUT_OF_RANGE/
);

let completed = false;
Promise.all(pending).then((outputs) => {
  for (let index = 0; index < outputs.length; index++) {
    assert.deepStrictEqual(
      outputs[index],
      inputs[index].map((value) => value * 2)
    );
  }
  completed = true;
});

process.on("beforeExit", () => {
  assert.strictEqual(completed, true);
});
