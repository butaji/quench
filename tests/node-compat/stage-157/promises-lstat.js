const fs = require("fs");
const assert = require("assert");

(async () => {
  const target = `/tmp/quench-node-stage-157-target-${process.pid}`;
  const link = `/tmp/quench-node-stage-157-link-${process.pid}`;
  fs.writeFileSync(target, "x");
  fs.symlinkSync(target, link);
  const stats = await fs.promises.lstat(link);
  assert.strictEqual(stats.isSymbolicLink(), true);
  fs.unlinkSync(link);
  fs.rmSync(target);
})().then(() => undefined);
