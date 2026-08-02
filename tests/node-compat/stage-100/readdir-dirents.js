const fs = require('fs');
const path = '/tmp/quench-node-stage-100';

fs.mkdirSync(path, { recursive: true });
fs.writeFileSync(`${path}/file`, 'x');
if (!fs.readdirSync(path).includes('file')) throw new Error('directory entry mismatch');
