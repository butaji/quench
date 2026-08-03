const fs = require("fs");
const assert = require("assert");

(async () => {
  const path = `/tmp/quench-node-stage-146-${process.pid}`;
  fs.writeFileSync(path, "a");
  const handle = await fs.promises.open(path, "a");
  await handle.appendFile("b");
  await handle.close();
  assert.strictEqual(fs.readFileSync(path, "utf8"), "ab");
  fs.rmSync(path);
})().then(() => undefined);
