const fs = require('fs');
const { text } = require('stream/iter');

(async () => {
  const path = `/tmp/quench-node-stage-171-${process.pid}`;
  fs.writeFileSync(path, 'abc');
  const handle = await fs.promises.open(path, 'r');
  const readable = handle.pull();
  try { handle.pull(); throw new Error('pull did not lock'); }
  catch (error) { if (error.code !== 'ERR_INVALID_STATE') throw error; }
  if (await text(readable) !== 'abc') throw new Error('pull state read mismatch');
  if (await text(handle.pull()) !== '') throw new Error('pull position mismatch');
  await handle.close();
  try { handle.pull(); throw new Error('closed pull accepted'); }
  catch (error) { if (error.code !== 'ERR_INVALID_STATE') throw error; }
})();
