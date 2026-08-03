const fs = require("fs");
const assert = require("assert");

(async () => {
  const path = `/tmp/quench-node-stage-166-${process.pid}`;
  const handle = await fs.promises.open(path, "w+");
  async function* chunks() {
    yield "a";
    yield Buffer.from("b");
    yield "c";
  }
  await handle.writeFile(chunks());
  await handle.close();
  assert.strictEqual(fs.readFileSync(path, "utf8"), "abc");
  fs.rmSync(path);
})();
