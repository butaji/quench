const fs = require("fs");
const assert = require("assert");

(async () => {
  const path = `/tmp/quench-node-stage-145-${process.pid}`;
  const handle = await fs.promises.open(path, "w+");
  await handle.writeFile("hello");
  assert.strictEqual(await handle.readFile("utf8"), "hello");
  await handle.close();
  fs.rmSync(path);
})().then(() => undefined);
