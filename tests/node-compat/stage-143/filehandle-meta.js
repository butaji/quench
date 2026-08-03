const fs = require("fs");
const assert = require("assert");

(async () => {
  const path = `/tmp/quench-node-stage-143-${process.pid}`;
  fs.writeFileSync(path, "abc");
  const handle = await fs.promises.open(path, "r");
  const stats = await handle.stat();
  await handle.sync();
  await handle.datasync();
  await handle.close();
  assert.strictEqual(stats.size, 3);
  assert.strictEqual(stats.isFile(), true);
  fs.rmSync(path);
})().then(() => undefined);
