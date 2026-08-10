const assert = require("assert");
const fs = require("fs");
const path = require("path");
const vfs = require("node:vfs");

(async () => {
  const base = fs.mkdtempSync(path.join(process.cwd(), "real-links-"));
  const root = path.join(base, "root");
  fs.mkdirSync(root);
  const filesystem = vfs.create(new vfs.RealFSProvider(root));
  try {
    await assert.rejects(
      filesystem.promises.symlink("/etc/passwd", "/escape"),
      { code: "EACCES" },
    );
    await assert.rejects(filesystem.promises.symlink("../../escape", "/bad"), {
      code: "EACCES",
    });
    fs.writeFileSync(path.join(root, "target.txt"), "x");
    fs.symlinkSync(path.join(root, "target.txt"), path.join(root, "abs"));
    assert.strictEqual(
      await filesystem.promises.readlink("/abs"),
      "/target.txt",
    );
    fs.symlinkSync(root, path.join(root, "root-link"));
    assert.strictEqual(await filesystem.promises.readlink("/root-link"), "/");
    fs.writeFileSync(path.join(base, "outside.txt"), "no");
    fs.symlinkSync(
      path.join(base, "outside.txt"),
      path.join(root, "outside-link"),
    );
    await assert.rejects(filesystem.promises.realpath("/outside-link"), {
      code: "EACCES",
    });
    console.log("real-provider symlink sequence passed");
  } finally {
    fs.rmSync(base, { recursive: true, force: true });
  }
})();
