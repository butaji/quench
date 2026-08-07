const assert = require("assert");
const { merge, text, bytes } = require("stream/iter");

(async () => {
  const arrayBuffer = new TextEncoder().encode("abc").buffer;
  const dataView = new DataView(new TextEncoder().encode("def").buffer);
  assert.strictEqual(await text(merge(arrayBuffer, dataView)), "abcdef");
  assert.deepStrictEqual(
    await bytes(arrayBuffer),
    new Uint8Array([97, 98, 99]),
  );
  const streamable = { toAsyncStream: async () => "ghi" };
  assert.strictEqual(await text(streamable), "ghi");
})();
