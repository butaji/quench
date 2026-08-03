const fs = require("fs");
const assert = require("assert");

(async () => {
  const path = `/tmp/quench-node-stage-165-${process.pid}`;
  const handle = await fs.promises.open(path, "w+");
  await handle.writeFile(["a", Buffer.from("b"), "c"]);
  await handle.close();
  assert.strictEqual(fs.readFileSync(path, "utf8"), "abc");
  fs.rmSync(path);
})();
