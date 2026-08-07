const { Readable } = require("stream");
const { test } = require("node:test");
test("readable state activates lazily", async () => {
  let called = false;
  const readable = new Readable({
    read() {
      called = true;
      this.push(null);
    },
  });
  if (readable._readableState.reading) {
    throw new Error("stream started reading early");
  }
  readable.on("data", () => {});
  await Promise.resolve();
  if (!called || !readable._readableState.reading) {
    throw new Error("read state was not updated");
  }
});
