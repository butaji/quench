const fs = require('fs');

const file = `/tmp/quench-node-stage-99-file-${process.pid}`;
const dir = `/tmp/quench-node-stage-99-dir-${process.pid}`;
fs.writeFileSync(file, 'x'); fs.mkdirSync(dir);
fs.rmSync(file);
if (fs.existsSync(file)) throw new Error('rm file failed');
try { fs.rmSync(dir, { recursive: false }); throw new Error('rm directory accepted'); }
catch (error) { if (error.code !== 'ERR_FS_EISDIR') throw error; }
