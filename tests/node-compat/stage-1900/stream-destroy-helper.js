const assert = require("assert");
const { Readable, Writable, destroy } = require("stream");

(async () => {
  for (
    const stream of [new Readable({ read() {} }), new Writable({ write() {} })]
  ) {
    destroy(stream);
    assert.strictEqual(stream.destroyed, true);
    await assert.rejects(
      new Promise((resolve, reject) => {
        stream.once("error", reject);
        stream.once("close", resolve);
      }),
      { name: "AbortError" },
    );
  }
  const stream = new Readable({ read() {} });
  destroy(stream, new Error("asd"));
  await assert.rejects(
    new Promise((resolve, reject) => {
      stream.once("error", reject);
      stream.once("close", resolve);
    }),
    { message: "asd" },
  );
})();
