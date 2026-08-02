const fs = require('fs');
const path = `/tmp/quench-node-stage-139-${process.pid}`;
fs.writeFileSync(path, 'mode');
if ((fs.statSync(path).mode & 0o777) !== (0o666 & ~process.umask())) throw new Error('default mode mismatch');
