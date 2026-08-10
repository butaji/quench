const assert = require("assert");
const fs = require("fs");
const path = require("path");
const vfs = require("node:vfs");

(async () => {
  const mount = path.resolve(`/tmp/vfs-promises-stage-${process.pid}`);
  const filesystem = vfs.create();
  filesystem.mkdirSync("/src", { recursive: true });
  filesystem.writeFileSync("/src/hello.txt", "hello world");
  filesystem.mount(mount);
  try {
    const file = path.join(mount, "src/hello.txt");
    assert.strictEqual((await fs.promises.stat(file)).isFile(), true);
    assert.strictEqual((await fs.promises.lstat(file)).isFile(), true);
    assert.ok(
      (await fs.promises.readdir(path.join(mount, "src"))).includes(
        "hello.txt",
      ),
    );
    assert.strictEqual(await fs.promises.readFile(file, "utf8"), "hello world");
    assert.strictEqual(await fs.promises.realpath(file), file);
    await fs.promises.access(file);
    const statfs = await fs.promises.statfs(file);
    assert.strictEqual(typeof statfs.bsize, "number");
    console.log("mounted fs promises path operations passed");
  } finally {
    filesystem.unmount();
  }
})();
