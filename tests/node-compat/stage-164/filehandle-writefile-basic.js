const fs = require("fs");
const assert = require("assert");

(async () => {
  const path = `/tmp/quench-node-stage-164-${process.pid}`;
  const handle = await fs.promises.open(path, "w+");
  await handle.writeFile("hello");
  await handle.close();
  assert.strictEqual(fs.readFileSync(path, "utf8"), "hello");
  fs.rmSync(path);
})();
