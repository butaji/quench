const assert = require("assert");
const { Readable } = require("stream");

const controller = new AbortController();
const pendingAbort = Readable.from([1, 2, 3]).reduce(
  async (total, value, { signal }) => {
    assert.strictEqual(signal.aborted, false);
    if (value === 3) await new Promise(() => {});
    return total + value;
  },
  0,
  { signal: controller.signal },
);

let completed = false;
Promise.all([
  Readable.from([1, 2, 3]).reduce((total, value) => total + value, 0),
  Readable.from([1, 2, 3])
    .map(async (value) => value)
    .reduce(async (total, value) => total + value),
  assert.rejects(
    Readable.from([]).reduce((left, right) => left + right),
    {
      code: "ERR_MISSING_ARGS",
    },
  ),
  assert.rejects(Readable.from([]).reduce(1), {
    code: "ERR_INVALID_ARG_TYPE",
  }),
  assert.rejects(pendingAbort, { name: "AbortError" }),
]).then(([sum, derivedSum]) => {
  assert.strictEqual(sum, 6);
  assert.strictEqual(derivedSum, 6);
  completed = true;
});

controller.abort();

process.on("beforeExit", () => {
  assert.strictEqual(completed, true);
});
