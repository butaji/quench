const fs = require('fs');

const path = `/tmp/quench-node-stage-133-${process.pid}`;
fs.writeFileSync(path, 'abcdef');
const fd = fs.openSync(path, 'r+');
fs.ftruncateSync(fd, 3);
if (fs.statSync(path).size !== 3) throw new Error('ftruncate sync mismatch');
fs.ftruncateSync(fd, 1);
if (fs.statSync(path).size !== 1) throw new Error('ftruncate mismatch');
fs.closeSync(fd);
