const fs = require("fs");
const assert = require("assert");

(async () => {
  const path = `/tmp/quench-node-stage-151-${process.pid}`;
  fs.writeFileSync(path, "abc");
  const fd = fs.openSync(path, "r+");
  const stats = await fs.promises.fstat(fd);
  await fs.promises.fchmod(fd, 0o600);
  fs.closeSync(fd);
  assert.strictEqual(stats.size, 3);
  assert.strictEqual(fs.statSync(path).mode & 0o777, 0o600);
  fs.rmSync(path);
})().then(() => undefined);
