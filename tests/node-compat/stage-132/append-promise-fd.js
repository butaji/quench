const fs = require("fs");
const assert = require("assert");

(async () => {
  const path = `/tmp/quench-node-stage-132-${process.pid}`;
  fs.writeFileSync(path, "a");
  const fd = fs.openSync(path, "a");
  await fs.promises.appendFile(fd, "b");
  fs.closeSync(fd);
  assert.strictEqual(fs.readFileSync(path, "utf8"), "ab");
  fs.rmSync(path);
})().then(() => undefined);
