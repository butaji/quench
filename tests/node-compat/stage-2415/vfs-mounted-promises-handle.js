const assert = require("assert");
const fs = require("fs");
const path = require("path");
const vfs = require("node:vfs");

(async () => {
  const mount = path.resolve(`/tmp/vfs-promises-handle-${process.pid}`);
  const filesystem = vfs.create();
  filesystem.writeFileSync("/hello.txt", "hello");
  filesystem.mount(mount);
  try {
    const handle = await fs.promises.open(path.join(mount, "hello.txt"), "r");
    assert.strictEqual(await handle.readFile("utf8"), "hello");
    await handle.close();
    console.log("mounted fs promises handle passed");
  } finally {
    filesystem.unmount();
  }
})();
