const fs = require("fs");
const assert = require("assert");

(async () => {
  const path = `/tmp/quench-node-stage-168-${process.pid}`;
  const handle = await fs.promises.open(path, "w+");
  await handle.writeFile(["ümlaut", " sechzig"], "latin1");
  await handle.close();
  assert.strictEqual(fs.readFileSync(path, "latin1"), "ümlaut sechzig");
  fs.rmSync(path);
})();
