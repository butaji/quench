const fs = require('fs');

(async () => {
  const path = '/tmp/quench-node-stage-105';
  fs.mkdirSync(path, { recursive: true });
  fs.writeFileSync(`${path}/file`, 'x');
  const entries = await fs.promises.readdir(path, { withFileTypes: true });
  if (!entries[0].isFile()) throw new Error('promise Dirent mismatch');
})().then(() => undefined);
