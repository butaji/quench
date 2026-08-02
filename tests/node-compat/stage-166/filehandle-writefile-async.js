const fs = require('fs');

(async () => {
  const path = `/tmp/quench-node-stage-166-${process.pid}`;
  const handle = await fs.promises.open(path, 'w+');
  async function* chunks() { yield 'a'; yield Buffer.from('b'); yield 'c'; }
  await handle.writeFile(chunks());
  await handle.close();
  if (fs.readFileSync(path, 'utf8') !== 'abc') throw new Error('async iterable mismatch');
})();
