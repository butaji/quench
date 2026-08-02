const fs = require('fs');
const path = `/tmp/quench-node-stage-126-${process.pid}`;
fs.writeFileSync(path, Uint8Array.from([0x61, 0x62, 0x63]));
if (fs.readFileSync(path, 'utf8') !== 'abc') throw new Error('typed-array write mismatch');
