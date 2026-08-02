const fs = require('fs');
const path = `/tmp/quench-node-stage-137-${process.pid}`;
fs.writeFileSync(path, 'hello');
const buffer = Buffer.alloc(8, 0x78);
const result = fs.readFileSync(path, { buffer });
if (result.toString() !== 'hello' || buffer[5] !== 0x78) throw new Error('read buffer option mismatch');
