const assert = require("assert");
const fs = require("fs");
const path = `/tmp/quench-node-stage-2404-${process.pid}`;

(async () => {
  const input = Buffer.alloc(300 * 1024, "x");
  fs.writeFileSync(path, input);
  const handle = await fs.promises.open(path, "r");
  try {
    const data = await require("stream/iter").bytes(
      handle.pull({ start: 50 * 1024, limit: 200 * 1024 })
    );
    assert.strictEqual(data.byteLength, 200 * 1024);
  } finally {
    await handle.close();
    fs.rmSync(path);
  }
  console.log("large ranged pull passed");
})();
