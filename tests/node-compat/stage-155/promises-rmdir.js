const fs = require('fs');

(async () => {
  const path = `/tmp/quench-node-stage-155-${process.pid}`;
  fs.mkdirSync(path);
  await fs.promises.rmdir(path);
  if (fs.existsSync(path)) throw new Error('promise rmdir mismatch');
})().then(() => undefined);
