const fs = require('fs');
const path = '/tmp/quench-node-stage-100';

fs.mkdirSync(path, { recursive: true });
fs.writeFileSync(`${path}/file`, 'x');
const entry = fs.readdirSync(path, { withFileTypes: true })[0];
if (!entry || !entry.isFile() || entry.isDirectory()) throw new Error('Dirent mismatch');
