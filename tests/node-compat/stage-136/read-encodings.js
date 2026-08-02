const fs = require('fs');
const path = `/tmp/quench-node-stage-136-${process.pid}`;
fs.writeFileSync(path, Buffer.from('hello'));
if (fs.readFileSync(path, 'hex') !== Buffer.from('hello').toString('hex')) throw new Error('hex read mismatch');
if (fs.readFileSync(path, { encoding: 'base64' }) !== Buffer.from('hello').toString('base64')) throw new Error('base64 read mismatch');
