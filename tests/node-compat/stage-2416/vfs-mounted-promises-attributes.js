const assert = require("assert");
const fs = require("fs");
const path = require("path");
const vfs = require("node:vfs");

(async () => {
  const mount = path.resolve(`/tmp/vfs-attributes-${process.pid}`);
  const filesystem = vfs.create();
  filesystem.writeFileSync("/file.txt", "hello");
  filesystem.symlinkSync("file.txt", "/link.txt");
  filesystem.mount(mount);
  const file = path.join(mount, "file.txt");
  const link = path.join(mount, "link.txt");
  try {
    const uid = process.getuid?.() ?? 0;
    const gid = process.getgid?.() ?? 0;
    const now = new Date();
    await fs.promises.chmod(file, 0o644);
    await fs.promises.chown(file, uid, gid);
    await fs.promises.lchown(file, uid, gid);
    await fs.promises.utimes(file, now, now);
    await fs.promises.lutimes(file, now, now);
    await fs.promises.lchmod(link, 0o700);
    assert.strictEqual((await fs.promises.lstat(file)).mode & 0o777, 0o644);
    assert.strictEqual((await fs.promises.lstat(link)).mode & 0o777, 0o700);
    console.log("mounted fs promise attributes passed");
  } finally {
    filesystem.unmount();
  }
})();
