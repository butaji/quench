const assert = require("assert");
const { merge } = require("stream/iter");

(async () => {
  let returned = 0;
  async function* source() {
    try {
      while (true) yield [new Uint8Array([1])];
    } finally {
      returned++;
    }
  }
  for await (const _ of merge(source(), source())) break;
  await new Promise(setImmediate);
  assert.strictEqual(returned, 2);

  const controller = new AbortController();
  const pending = {
    async next() {
      return new Promise(() => {});
    },
    async return() {
      returned++;
      return { done: true };
    },
    [Symbol.asyncIterator]() {
      return this;
    },
  };
  const next = merge(pending, { signal: controller.signal })
    [Symbol.asyncIterator]().next();
  controller.abort();
  await assert.rejects(next, { name: "AbortError" });
})();
