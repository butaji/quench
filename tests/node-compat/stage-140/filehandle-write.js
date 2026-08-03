const fs = require("fs");
const assert = require("assert");

(async () => {
  const path = `/tmp/quench-node-stage-140-${process.pid}`;
  const handle = await fs.promises.open(path, "w+");
  const result = await handle.write(Buffer.from("abcd"), 1, 2, 0);
  await handle.close();
  assert.strictEqual(result.bytesWritten, 2);
  assert.strictEqual(fs.readFileSync(path, "utf8"), "bc");
  fs.rmSync(path);
})().then(() => undefined);
