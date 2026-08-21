const assert = require("assert");
const fs = require("fs");
const path = require("path");
const vfs = require("node:vfs");

(async () => {
  const mount = path.resolve(`/tmp/vfs-attr-sequence-${process.pid}`);
  const filesystem = vfs.create();
  filesystem.mkdirSync("/src", { recursive: true });
  filesystem.writeFileSync("/src/hello.txt", "hello world");
  filesystem.mount(mount);
  const p = (name) => path.join(mount, "src", name);
  try {
    await fs.promises.writeFile(p("pw.txt"), "pdata");
    await fs.promises.appendFile(p("pw.txt"), " more");
    await fs.promises.rm(p("pw.txt"));
    await fs.promises.copyFile(p("hello.txt"), p("copy.txt"));
    await fs.promises.rename(p("copy.txt"), p("renamed.txt"));
    await fs.promises.unlink(p("renamed.txt"));
    await fs.promises.symlink("hello.txt", p("plnk.txt"));
    await fs.promises.truncate(p("hello.txt"), 5);
    await fs.promises.link(p("hello.txt"), p("plink.txt"));
    await fs.promises.mkdtemp(p("ptmp-"));
    await fs.promises.chmod(p("hello.txt"), 0o644);
    assert.strictEqual(
      (await fs.promises.lstat(p("hello.txt"))).mode & 0o777,
      0o644,
    );
    console.log("mounted promise attribute sequence passed");
  } finally {
    filesystem.unmount();
  }
})();
