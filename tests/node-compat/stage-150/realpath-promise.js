const fs = require("fs");
const assert = require("assert");

(async () => {
  const path = `/tmp/quench-node-stage-150-${process.pid}`;
  fs.writeFileSync(path, "x");
  assert.strictEqual(await fs.promises.realpath(path), fs.realpathSync(path));
  const result = await fs.promises.realpath(path, { encoding: "buffer" });
  assert.strictEqual(Buffer.isBuffer(result), true);
  assert.strictEqual(result.toString(), fs.realpathSync(path));
  fs.rmSync(path);
})().then(() => undefined);
