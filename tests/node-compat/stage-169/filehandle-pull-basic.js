const fs = require('fs');
const { text, bytes } = require('stream/iter');

(async () => {
  const path = `/tmp/quench-node-stage-169-${process.pid}`;
  fs.writeFileSync(path, 'hello from pull');
  const handle = await fs.promises.open(path, 'r');
  if (await text(handle.pull()) !== 'hello from pull') throw new Error('pull text mismatch');
  await handle.close();
  const binary = await fs.promises.open(path, 'r');
  if ((await bytes(binary.pull())).byteLength !== 15) throw new Error('pull bytes mismatch');
  await binary.close();
})();
