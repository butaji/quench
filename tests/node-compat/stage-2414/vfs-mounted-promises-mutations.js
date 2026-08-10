const assert = require("assert");
const fs = require("fs");
const path = require("path");
const vfs = require("node:vfs");

(async () => {
  const mount = path.resolve(`/tmp/vfs-promises-mutation-${process.pid}`);
  const filesystem = vfs.create();
  filesystem.mkdirSync("/src", { recursive: true });
  filesystem.writeFileSync("/src/hello.txt", "hello world");
  filesystem.mount(mount);
  const file = (name) => path.join(mount, "src", name);
  try {
    await fs.promises.writeFile(file("pw.txt"), "pdata");
    await fs.promises.appendFile(file("pw.txt"), " more");
    assert.strictEqual(fs.readFileSync(file("pw.txt"), "utf8"), "pdata more");
    await fs.promises.mkdir(file("pd"));
    await fs.promises.rmdir(file("pd"));
    await fs.promises.rm(file("pw.txt"));
    await fs.promises.copyFile(file("hello.txt"), file("copy.txt"));
    await fs.promises.rename(file("copy.txt"), file("renamed.txt"));
    await fs.promises.unlink(file("renamed.txt"));
    await fs.promises.symlink("hello.txt", file("link.txt"));
    assert.strictEqual(
      await fs.promises.readlink(file("link.txt")),
      "hello.txt",
    );
    await fs.promises.truncate(file("hello.txt"), 5);
    assert.strictEqual(fs.readFileSync(file("hello.txt"), "utf8"), "hello");
    console.log("mounted fs promises mutations passed");
  } finally {
    filesystem.unmount();
  }
})();
