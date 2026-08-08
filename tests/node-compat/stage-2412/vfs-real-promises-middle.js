const assert = require("assert");
const fs = require("fs");
const path = require("path");
const vfs = require("node:vfs");

(async () => {
  const root = fs.mkdtempSync(path.join(process.cwd(), "real-promises-mid-"));
  const filesystem = vfs.create(new vfs.RealFSProvider(root));
  try {
    await filesystem.promises.writeFile("/a.txt", "hello");
    const stat = await filesystem.promises.stat("/a.txt");
    assert.strictEqual(stat.size, 5);
    assert.strictEqual(
      (await filesystem.promises.lstat("/a.txt")).isFile(),
      true
    );
    await filesystem.promises.access("/a.txt");
    await assert.rejects(filesystem.promises.access("/missing.txt"), {
      code: "ENOENT"
    });
    await filesystem.promises.mkdir("/d/sub", { recursive: true });
    assert.deepStrictEqual((await filesystem.promises.readdir("/d")).sort(), [
      "sub"
    ]);
    await filesystem.promises.rmdir("/d/sub");
    await filesystem.promises.writeFile("/old.txt", "x");
    await filesystem.promises.rename("/old.txt", "/new.txt");
    assert.strictEqual(filesystem.existsSync("/old.txt"), false);
    await filesystem.promises.unlink("/new.txt");
    await filesystem.promises.copyFile("/a.txt", "/copy.txt");
    assert.strictEqual(
      await filesystem.promises.readFile("/copy.txt", "utf8"),
      "hello"
    );
    await assert.rejects(filesystem.provider.open("/missing.txt", "r"), {
      code: "ENOENT"
    });
    console.log("real-provider promise middle sequence passed");
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
})();
