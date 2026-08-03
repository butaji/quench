const fs = require("fs");
const assert = require("assert");

(async () => {
  const path = `/tmp/quench-node-stage-142-${process.pid}`;
  fs.writeFileSync(path, "abcdef");
  const handle = await fs.promises.open(path, "r+");
  await handle.truncate(2);
  await handle.close();
  assert.strictEqual(fs.readFileSync(path, "utf8"), "ab");
  fs.rmSync(path);
})().then(() => undefined);
