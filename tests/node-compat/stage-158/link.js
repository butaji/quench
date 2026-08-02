const fs = require('fs');

(async () => {
  const source = `/tmp/quench-node-stage-158-source-${process.pid}`;
  const target = `/tmp/quench-node-stage-158-target-${process.pid}`;
  fs.writeFileSync(source, 'hard');
  fs.linkSync(source, target);
  if (fs.readFileSync(target, 'utf8') !== 'hard') throw new Error('link sync mismatch');
  await fs.promises.unlink(target);
  await new Promise((resolve, reject) => fs.link(source, target, (error) => error ? reject(error) : resolve()));
  await fs.promises.unlink(target);
  fs.unlinkSync(source);
})().then(() => undefined);
