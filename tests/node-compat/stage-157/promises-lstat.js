const fs = require('fs');

(async () => {
  const target = `/tmp/quench-node-stage-157-target-${process.pid}`;
  const link = `/tmp/quench-node-stage-157-link-${process.pid}`;
  fs.writeFileSync(target, 'x');
  fs.symlinkSync(target, link);
  const stats = await fs.promises.lstat(link);
  if (!stats.isSymbolicLink()) throw new Error('promise lstat mismatch');
})().then(() => undefined);
