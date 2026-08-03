const fs = require("fs");
const assert = require("assert");

(async () => {
  const prefix = `/tmp/quench-node-stage-156-${process.pid}-`;
  const path = await fs.promises.mkdtemp(prefix);
  assert.strictEqual(path.startsWith(prefix), true);
  assert.strictEqual(fs.statSync(path).isDirectory(), true);
  fs.rmdirSync(path);
})().then(() => undefined);
