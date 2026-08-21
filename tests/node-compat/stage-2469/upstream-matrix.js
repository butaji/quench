const assert = require("assert");
const { setTimeout: delay } = require("timers/promises");
const { Readable } = require("stream");

const oneTo5 = () => Readable.from([1, 2, 3, 4, 5]);
const oneTo5Async = () => oneTo5().map(async (value) => value);

const countCalls = (predicate, expected) => {
  let calls = 0;
  const callback = async (value) => {
    calls++;
    return predicate(value);
  };
  callback.check = () => assert.strictEqual(calls, expected);
  return callback;
};

const runPredicates = async () => {
  assert.strictEqual(await oneTo5().some((value) => value > 3), true);
  assert.strictEqual(await oneTo5().every((value) => value > 3), false);
  assert.strictEqual(await oneTo5().find((value) => value > 3), 4);
  assert.strictEqual(await oneTo5Async().find(async (value) => value > 3), 4);

  for (
    const [method, predicate, expectedCalls] of [
      ["some", (value) => value > 2, 3],
      ["every", (value) => value < 3, 3],
      ["find", (value) => value > 1, 2],
    ]
  ) {
    const stream = oneTo5Async();
    const callback = countCalls(predicate, expectedCalls);
    await stream[method](callback);
    callback.check();
    await delay();
    assert.strictEqual(stream.destroyed, true);
  }
};

const runAbortValidation = async () => {
  const found = await Readable.from([1, 2]).find(
    async (value) => {
      if (value === 1) await delay(10);
      return true;
    },
    { concurrency: 2 },
  );
  assert.strictEqual(found, 1);

  for (const method of ["some", "every", "find"]) {
    const controller = new AbortController();
    const pending = Readable.from([1, 2, 3])[method](
      () => new Promise(() => {}),
      { signal: controller.signal },
    );
    controller.abort();
    await assert.rejects(pending, { name: "AbortError" });
    await assert.rejects(
      Readable.from([1, 2, 3])[method](() => new Promise(() => {}), {
        signal: AbortSignal.abort(),
      }),
      { name: "AbortError" },
    );
    await assert.rejects(Readable.from([1])[method](1), {
      code: "ERR_INVALID_ARG_TYPE",
    });
    await assert.rejects(
      Readable.from([1])[method]((value) => value, {
        concurrency: "invalid",
      }),
      { code: "ERR_OUT_OF_RANGE" },
    );
  }
};

const run = async () => {
  await runPredicates();
  await runAbortValidation();
};

let failure;
let completed = false;
const keepAlive = setInterval(() => {}, 10);
run().then(
  () => {
    completed = true;
    clearInterval(keepAlive);
  },
  (error) => {
    failure = error;
    completed = true;
    clearInterval(keepAlive);
  },
);

process.on("beforeExit", () => {
  if (failure) throw failure;
  assert.strictEqual(completed, true);
});
