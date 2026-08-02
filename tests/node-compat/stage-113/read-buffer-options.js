const fs = require('fs');

const path = `/tmp/quench-node-stage-113-${process.pid}`;
fs.writeFileSync(path, 'x');
const fd = fs.openSync(path, 'r');
const buffer = Buffer.alloc(1);
fs.read(fd, { buffer, offset: null }, (error, count, result) => {
  if (error || count !== 1 || result[0] !== 120) throw error || new Error('buffer options mismatch');
  fs.closeSync(fd);
});
