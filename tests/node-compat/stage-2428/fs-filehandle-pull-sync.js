const assert = require("assert");
const fs = require("fs");

(async () => {
  const path = `/tmp/pull-sync-${process.pid}.txt`;
  fs.writeFileSync(path, "hello sync");
  const handle = await fs.promises.open(path, "r");
  const batches = handle.pullSync();
  const values = [...batches];
  assert.strictEqual(Buffer.concat(values.flat()).toString(), "hello sync");
  assert.ok((await handle.stat()).size > 0);
  await handle.close();
  console.log("FileHandle pullSync passed");
})();
