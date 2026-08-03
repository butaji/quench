const fs = require("fs");
const assert = require("assert");

(async () => {
  const path = `/tmp/quench-node-stage-167-${process.pid}`;
  const handle = await fs.promises.open(path, "w+");
  try {
    await handle.writeFile(42);
    assert.fail("accepted invalid writeFile value");
  } catch (error) {
    assert.strictEqual(error.code, "ERR_INVALID_ARG_TYPE");
  }
  await handle.close();
  fs.rmSync(path);
})();
