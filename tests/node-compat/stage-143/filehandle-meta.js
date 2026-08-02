const fs = require('fs');

(async () => {
  const path = `/tmp/quench-node-stage-143-${process.pid}`;
  fs.writeFileSync(path, 'abc');
  const handle = await fs.promises.open(path, 'r');
  const stats = await handle.stat();
  await handle.sync();
  await handle.datasync();
  await handle.close();
  if (stats.size !== 3 || !stats.isFile()) throw new Error('filehandle metadata mismatch');
})().then(() => undefined);
