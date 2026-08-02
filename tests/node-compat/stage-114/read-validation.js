const fs = require('fs');
const path = `/tmp/quench-node-stage-114-${process.pid}`;
fs.writeFileSync(path, 'x');
const fd = fs.openSync(path, 'r');
try { fs.readSync(fd, Buffer.alloc(1), -1, 1, 0); throw new Error('accepted invalid offset'); }
catch (error) { if (error.code !== 'ERR_OUT_OF_RANGE') throw error; }
try { fs.read(fd, Buffer.alloc(1), 0, 1, 'bad', () => {}); throw new Error('accepted invalid position'); }
catch (error) { if (error.code !== 'ERR_INVALID_ARG_TYPE') throw error; }
fs.closeSync(fd);
