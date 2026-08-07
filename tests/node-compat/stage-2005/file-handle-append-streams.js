const assert = require("assert");
const fs = require("fs");
const fsp = require("fs/promises");
const { Readable } = require("stream");

(async () => {
  const path = "stage-2005-append.txt";
  const handle = await fsp.open(path, "a");
  await handle.appendFile(Readable.from(["a", "b"]));
  await handle.appendFile({
    async *[Symbol.asyncIterator]() {
      yield "c";
      yield "d";
    },
  });
  await handle.close();
  assert.strictEqual(fs.readFileSync(path, "utf8"), "abcd");
  fs.unlinkSync(path);

  const abortPath = "stage-2005-abort.txt";
  const abortHandle = await fsp.open(abortPath, "a");
  const controller = new AbortController();
  controller.abort();
  await assert.rejects(
    abortHandle.appendFile("x", { signal: controller.signal }),
    { name: "AbortError" },
  );
  await abortHandle.close();
  fs.unlinkSync(abortPath);
  console.log("file handle append streams passed");
})().catch((error) => {
  console.error(error);
  throw error;
});
