const assert = require("assert");
const fs = require("fs").promises;

(async () => {
  const handle = await fs.open("stage-1817-file.txt", "w+");
  await handle.close();
  assert.strictEqual(handle.fd, -1);
  await assert.rejects(() => handle.stat(), { code: "EBADF" });
  await fs.unlink("stage-1817-file.txt");
  console.log("FileHandle close state passed");
})();
