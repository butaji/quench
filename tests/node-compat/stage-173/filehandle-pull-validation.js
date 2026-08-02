const fs = require('fs');

(async () => {
  const handle = await fs.promises.open(`/tmp/quench-node-stage-173-${process.pid}`, 'w+');
  for (const options of [{ autoClose: 'no' }, { signal: {} }, { start: 'a' }, { limit: 1.1 }, { chunkSize: 1.1 }]) {
    try { handle.pull(options); throw new Error('accepted invalid pull option'); }
    catch (error) { if (!['ERR_INVALID_ARG_TYPE', 'ERR_OUT_OF_RANGE'].includes(error.code)) throw error; }
  }
  await handle.close();
})();
