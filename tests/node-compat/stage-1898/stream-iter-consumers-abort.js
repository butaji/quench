const assert = require("assert");
const { from, bytes, text, array, arrayBuffer, toAsyncStreamable } = require(
  "stream/iter",
);

(async () => {
  for (
    const [name, consumer] of [["bytes", bytes], ["text", text], [
      "array",
      array,
    ], ["arrayBuffer", arrayBuffer]]
  ) {
    const controller = new AbortController();
    const reason = new Error(`${name} boom`);
    const promise = consumer(
      (async function* () {
        await new Promise(() => {});
      })(),
      { signal: controller.signal },
    );
    controller.abort(reason);
    await assert.rejects(promise, reason);
  }
  const controller = new AbortController();
  const source = {
    [toAsyncStreamable]() {
      return new Promise(() => {});
    },
  };
  const promise = bytes(source, { signal: controller.signal });
  controller.abort(new Error("normalization boom"));
  await assert.rejects(promise, { message: "normalization boom" });
})();
