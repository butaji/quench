const fs = require("fs");
const assert = require("assert");
const { text } = require("stream/iter");

(async () => {
  const path = `/tmp/quench-node-stage-171-${process.pid}`;
  fs.writeFileSync(path, "abc");
  const handle = await fs.promises.open(path, "r");
  const readable = handle.pull();
  try {
    handle.pull();
    assert.fail("pull did not lock");
  } catch (error) {
    assert.strictEqual(error.code, "ERR_INVALID_STATE");
  }
  assert.strictEqual(await text(readable), "abc");
  assert.strictEqual(await text(handle.pull()), "");
  await handle.close();
  try {
    handle.pull();
    assert.fail("closed pull accepted");
  } catch (error) {
    assert.strictEqual(error.code, "ERR_INVALID_STATE");
  }
  fs.rmSync(path);
})();
