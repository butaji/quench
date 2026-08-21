const assert = require("assert");
const fs = require("fs");
const path = require("path");
const vfs = require("node:vfs");

(async () => {
  const root = fs.mkdtempSync(path.join(process.cwd(), "real-link-"));
  const provider = new vfs.RealFSProvider(root);
  const filesystem = vfs.create(provider);
  try {
    await assert.rejects(
      filesystem.promises.symlink("/etc/passwd", "/escape"),
      {
        code: "EACCES",
      },
    );
    fs.writeFileSync(path.join(root, "target.txt"), "x");
    fs.symlinkSync(path.join(root, "target.txt"), path.join(root, "link"));
    assert.strictEqual(
      await filesystem.promises.readlink("/link"),
      "/target.txt",
    );
    assert.strictEqual(
      await filesystem.promises.realpath("/link"),
      "/target.txt",
    );
    console.log("async real-provider symlink contracts passed");
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
})();
