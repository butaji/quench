const assert = require("assert");
const { merge } = require("stream/iter");

const errorMessage = async (promise) => {
  try {
    await promise;
  } catch (error) {
    return error;
  }
  throw new Error("expected rejection");
};

(async () => {
  async function* bad() {
    yield [new Uint8Array([1])];
    throw new Error("source boom");
  }
  const sourceError = await errorMessage((async () => {
    for await (const _ of merge(bad())) {}
  })());
  assert.strictEqual(sourceError.message, "source boom");

  async function* cleanup() {
    try {
      yield [new Uint8Array([1])];
    } finally {
      throw new Error("cleanup boom");
    }
  }
  const cleanupError = await errorMessage((async () => {
    for await (const _ of merge(cleanup())) {}
  })());
  assert.strictEqual(cleanupError.message, "cleanup boom");
})();
