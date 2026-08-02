const fs = require('fs');
const path = `/tmp/quench-node-stage-125-${process.pid}`;
const fd = fs.openSync(path, 'w+');
fs.writeFileSync(fd, 'via-fd');
fs.closeSync(fd);
if (fs.readFileSync(path, 'utf8') !== 'via-fd') throw new Error('writeFileSync fd mismatch');
