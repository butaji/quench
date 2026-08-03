const fs = require("fs");
const assert = require("assert");

(async () => {
  const target = `/tmp/quench-node-stage-109-target-${process.pid}`;
  const link = `/tmp/quench-node-stage-109-link-${process.pid}`;
  fs.writeFileSync(target, "x");
  await fs.promises.symlink(target, link, "file");
  assert.strictEqual(await fs.promises.readlink(link), target);
  assert.strictEqual((await fs.promises.lstat(link)).isSymbolicLink(), true);
  await fs.promises.unlink(link);
  await fs.promises.rm(target);
})().then(() => undefined);
