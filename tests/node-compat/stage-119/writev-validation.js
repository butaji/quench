const fs = require('fs');
const path = `/tmp/quench-node-stage-119-${process.pid}`;
const fd = fs.openSync(path, 'w');
try { fs.writev(fd, {}, null, () => {}); throw new Error('accepted invalid buffers'); }
catch (error) { if (error.code !== 'ERR_INVALID_ARG_TYPE') throw error; }
fs.closeSync(fd);
