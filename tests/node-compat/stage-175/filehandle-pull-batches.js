const fs = require('fs');

(async () => {
  const path = `/tmp/quench-node-stage-175-${process.pid}`;
  fs.writeFileSync(path, 'abcdefghij');
  const handle = await fs.promises.open(path, 'r');
  const batches = [];
  for await (const batch of handle.pull({ chunkSize: 2 })) {
    if (!Array.isArray(batch) || batch.length !== 1 || batch[0].byteLength > 2) throw new Error('invalid pull batch');
    batches.push(batch[0]);
  }
  if (batches.length !== 5) throw new Error('pull batch count mismatch');
  await handle.close();
})();
