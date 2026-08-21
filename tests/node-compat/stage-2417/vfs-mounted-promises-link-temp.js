const assert = require("assert");
const fs = require("fs");
const path = require("path");
const vfs = require("node:vfs");

(async () => {
  const mount = path.resolve(`/tmp/vfs-link-temp-${process.pid}`);
  const filesystem = vfs.create();
  filesystem.mkdirSync("/src", { recursive: true });
  filesystem.writeFileSync("/src/file.txt", "hello");
  filesystem.mount(mount);
  try {
    const source = path.join(mount, "src/file.txt");
    const hard = path.join(mount, "src/hard.txt");
    await fs.promises.link(source, hard);
    assert.strictEqual(fs.readFileSync(hard, "utf8"), "hello");
    const temp = await fs.promises.mkdtemp(path.join(mount, "src/tmp-"));
    assert.ok(temp.startsWith(path.join(mount, "src/tmp-")));
    assert.strictEqual(fs.statSync(temp).isDirectory(), true);
    console.log("mounted fs promise link and mkdtemp passed");
  } finally {
    filesystem.unmount();
  }
})();
