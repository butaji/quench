const fs = require("fs");
const assert = require("assert");

(async () => {
  const path = `/tmp/quench-node-stage-144-${process.pid}`;
  const handle = await fs.promises.open(path, "w+");
  await handle.chmod(0o600);
  assert.strictEqual(fs.statSync(path).mode & 0o777, 0o600);
  await handle.close();
  try {
    await handle.stat();
    assert.fail("closed handle remained usable");
  } catch (error) {
    assert.strictEqual(error.code, "EBADF");
  }
  fs.rmSync(path);
})().then(() => undefined);
