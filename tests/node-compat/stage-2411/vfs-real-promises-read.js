const assert = require("assert");
const fs = require("fs");
const path = require("path");
const vfs = require("node:vfs");

(async () => {
  const root = fs.mkdtempSync(path.join(process.cwd(), "real-promises-"));
  const filesystem = vfs.create(new vfs.RealFSProvider(root));
  try {
    await filesystem.promises.writeFile("/a.txt", "hello");
    assert.strictEqual(
      await filesystem.promises.readFile("/a.txt", "utf8"),
      "hello",
    );
    console.log("real-provider promise read/write passed");
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
})();
