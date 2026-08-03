const fs = require("fs");
const assert = require("assert");

(async () => {
  const path = `/tmp/quench-node-stage-155-${process.pid}`;
  fs.mkdirSync(path);
  await fs.promises.rmdir(path);
  assert.strictEqual(fs.existsSync(path), false);
})().then(() => undefined);
