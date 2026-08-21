const assert = require("assert");
const fs = require("fs");
const { textSync } = require("stream/iter");

(async () => {
  const path = `/tmp/pullsync-iter-${process.pid}.txt`;
  fs.writeFileSync(path, "hello from sync file read");
  const handle = await fs.promises.open(path, "r");
  assert.strictEqual(textSync(handle.pullSync()), "hello from sync file read");
  await handle.close();
  console.log("pullSync stream iter passed");
})();
