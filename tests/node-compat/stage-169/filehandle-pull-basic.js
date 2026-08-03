const fs = require("fs");
const assert = require("assert");
const { text, bytes } = require("stream/iter");

(async () => {
  const path = `/tmp/quench-node-stage-169-${process.pid}`;
  fs.writeFileSync(path, "hello from pull");
  const handle = await fs.promises.open(path, "r");
  assert.strictEqual(await text(handle.pull()), "hello from pull");
  await handle.close();
  const binary = await fs.promises.open(path, "r");
  assert.strictEqual((await bytes(binary.pull())).byteLength, 15);
  await binary.close();
  fs.rmSync(path);
})();
