const assert = require("assert");
const { Readable } = require("stream");

const controller = new AbortController();
const liveTake = Readable.from([1, 2, 3])
  .take(1, { signal: controller.signal })
  .toArray();
const liveDrop = Readable.from([1, 2, 3])
  .drop(1, { signal: controller.signal })
  .toArray();
controller.abort();

const preAborted = Readable.from([1, 2, 3])
  .take(1, { signal: AbortSignal.abort() })
  .toArray();

let settled;
Promise.allSettled([liveTake, liveDrop, preAborted]).then((results) => {
  settled = results;
});

process.on("beforeExit", () => {
  assert.strictEqual(settled?.length, 3);
  for (const result of settled) {
    assert.strictEqual(result.status, "rejected");
    assert.strictEqual(result.reason.name, "AbortError");
  }
});
