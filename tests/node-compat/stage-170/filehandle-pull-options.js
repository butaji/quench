const fs = require("fs");
const assert = require("assert");
const { text } = require("stream/iter");

(async () => {
  const path = `/tmp/quench-node-stage-170-${process.pid}`;
  fs.writeFileSync(path, "AAABBBCCCDDD");
  const handle = await fs.promises.open(path, "r");
  assert.strictEqual(
    await text(handle.pull({ start: 3, limit: 3, chunkSize: 1 })),
    "BBB",
  );
  await handle.close();
  fs.rmSync(path);
})();
