const fs = require('fs');
const path = `/tmp/quench-node-stage-130-${process.pid}`;
for (const value of ['true', 0, [], {}]) {
  try { fs.writeFileSync(path, 'x', { flush: value }); throw new Error('accepted invalid flush'); }
  catch (error) { if (error.code !== 'ERR_INVALID_ARG_TYPE') throw error; }
}
fs.writeFileSync(path, 'flushed', { flush: true });
if (fs.readFileSync(path, 'utf8') !== 'flushed') throw new Error('flush write mismatch');
