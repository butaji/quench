const fs = require("fs");
const assert = require("assert");

(async () => {
  const path = `/tmp/quench-node-stage-153-${process.pid}`;
  fs.writeFileSync(path, "mode");
  await fs.promises.chmod(path, 0o600);
  assert.strictEqual(fs.statSync(path).mode & 0o777, 0o600);
  fs.rmSync(path);
})().then(() => undefined);
