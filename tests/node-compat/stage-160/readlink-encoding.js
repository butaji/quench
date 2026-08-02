const fs = require('fs');

(async () => {
  const target = `/tmp/quench-node-stage-160-target-${process.pid}`;
  const link = `/tmp/quench-node-stage-160-link-${process.pid}`;
  fs.writeFileSync(target, 'x');
  fs.symlinkSync(target, link);
  const expected = fs.readlinkSync(link);
  if (fs.readlinkSync(link, 'utf8') !== expected) throw new Error('readlink utf8 mismatch');
  if (fs.readlinkSync(link, 'buffer').toString() !== expected) throw new Error('readlink buffer mismatch');
})().then(() => undefined);
