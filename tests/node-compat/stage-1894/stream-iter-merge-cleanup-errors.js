const assert = require("assert");
const { merge } = require("stream/iter");

(async () => {
  async function* failingReturn() {
    try {
      yield [new Uint8Array([1])];
    } finally {
      throw new Error("cleanup boom");
    }
  }
  await assert.rejects(
    (async () => {
      for await (const _ of merge(failingReturn())) break;
    })(),
    { message: "cleanup boom" },
  );
})();
