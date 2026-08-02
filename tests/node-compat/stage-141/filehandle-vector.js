const fs = require('fs');

(async () => {
  const path = `/tmp/quench-node-stage-141-${process.pid}`;
  const handle = await fs.promises.open(path, 'w+');
  const written = await handle.writev([Buffer.from('ab'), Buffer.from('cd')], 0);
  if (written.bytesWritten !== 4) throw new Error('filehandle writev mismatch');
  const buffers = [Buffer.alloc(2), Buffer.alloc(2)];
  const read = await handle.readv(buffers, 0);
  await handle.close();
  if (read.bytesRead !== 4 || Buffer.concat(read.buffers).toString() !== 'abcd') throw new Error('filehandle readv mismatch');
})().then(() => undefined);
