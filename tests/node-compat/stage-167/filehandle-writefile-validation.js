const fs = require('fs');

(async () => {
  const handle = await fs.promises.open(`/tmp/quench-node-stage-167-${process.pid}`, 'w+');
  try { await handle.writeFile(42); throw new Error('accepted invalid writeFile value'); }
  catch (error) { if (error.code !== 'ERR_INVALID_ARG_TYPE') throw error; }
  await handle.close();
})();
