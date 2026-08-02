const fs = require('fs');

const path = '/tmp/quench-node-stage-106';
fs.mkdirSync(path, { recursive: true });
fs.writeFileSync(`${path}/file`, 'x');
const dir = fs.opendirSync(path);
if (!dir.readSync().isFile()) throw new Error('opendir Dirent mismatch');
dir.closeSync();
