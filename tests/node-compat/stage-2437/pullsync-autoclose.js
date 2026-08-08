const assert = require("assert");
const fs = require("fs");

(async () => {
  const path = `/tmp/pullsync-close-${process.pid}.txt`;
  fs.writeFileSync(path, "x".repeat(100000));
  const handle = await fs.promises.open(path, "r");
  for (const batch of handle.pullSync({ autoClose: true })) {
    assert.ok(batch.length > 0);
    break;
  }
  await assert.rejects(handle.stat(), { code: "EBADF" });
  console.log("pullSync early autoClose passed");
})();
