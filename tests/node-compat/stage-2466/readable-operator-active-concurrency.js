const assert = require("assert");
const { once } = require("events");
const { Readable } = require("stream");

const dependentPromises = Array.from({ length: 4 }, () => {
  let resolve;
  const promise = new Promise((settle) => {
    resolve = settle;
  });
  return { promise, resolve };
});

for (let index = 1; index < dependentPromises.length; index++) {
  const previous = dependentPromises[index - 1];
  const current = dependentPromises[index];
  const resolve = current.resolve;
  current.resolve = () => previous.promise.then(resolve);
}

const finishOrder = [];
const stream = Readable.from([2, 0, 1, 3]).map(
  async (value) => {
    const current = dependentPromises[value];
    current.resolve();
    await current.promise;
    finishOrder.push(value);
    return value;
  },
  { concurrency: 2 }
);

let output;
stream.toArray().then((values) => {
  output = values;
});

const controller = new AbortController();
let abortCalls = 0;
let abortCompleted = false;
const aborted = Readable.from([1, 2, 3, 4]).filter(
  async (_value, { signal }) => {
    abortCalls++;
    await once(signal, "abort");
  },
  { concurrency: 2, signal: controller.signal }
);
assert
  .rejects(aborted.toArray(), { name: "AbortError" })
  .then(() => (abortCompleted = true));
setImmediate(() => controller.abort());

process.on("beforeExit", () => {
  assert.deepStrictEqual(output, [2, 0, 1, 3]);
  assert.deepStrictEqual(finishOrder, [0, 1, 2, 3]);
  assert.strictEqual(abortCalls, 2);
  assert.strictEqual(abortCompleted, true);
});
