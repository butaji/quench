const fs = require('fs');

(async () => {
  const path = `/tmp/quench-node-stage-165-${process.pid}`;
  const handle = await fs.promises.open(path, 'w+');
  await handle.writeFile(['a', Buffer.from('b'), 'c']);
  await handle.close();
  if (fs.readFileSync(path, 'utf8') !== 'abc') throw new Error('filehandle iterable mismatch');
})();
