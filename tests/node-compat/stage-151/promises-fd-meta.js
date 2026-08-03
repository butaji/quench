const fs = require("fs");

(async () => {
  const path = `/tmp/quench-node-stage-151-${process.pid}`;
  fs.writeFileSync(path, "abc");
  const fd = fs.openSync(path, "r+");
  const stats = await fs.promises.fstat(fd);
  await fs.promises.fchmod(fd, 0o600);
  fs.closeSync(fd);
  if (stats.size !== 3 || (fs.statSync(path).mode & 0o777) !== 0o600)
    throw new Error("promise fd metadata mismatch");
})().then(() => undefined);
