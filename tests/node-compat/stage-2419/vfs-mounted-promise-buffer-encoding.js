const assert = require("assert");
const fs = require("fs");
const path = require("path");
const vfs = require("node:vfs");

(async () => {
  let counter = 0;
  const base = path.resolve(`/tmp/vfs-buffer-${process.pid}`);
  const mounted = () => {
    const mount = `${base}-${counter++}`;
    const filesystem = vfs.create();
    filesystem.mkdirSync("/src", { recursive: true });
    filesystem.writeFileSync("/src/hello.txt", "hello");
    filesystem.mount(mount);
    return { filesystem, mount };
  };
  {
    const { filesystem, mount } = mounted();
    const entries = await fs.promises.readdir(path.join(mount, "src"), {
      encoding: "buffer",
    });
    assert.ok(entries.every(Buffer.isBuffer));
    filesystem.unmount();
  }
  {
    const { filesystem, mount } = mounted();
    const target = path.join(mount, "src/hello.txt");
    const result = await fs.promises.realpath(target, { encoding: "buffer" });
    assert.ok(Buffer.isBuffer(result));
    assert.strictEqual(result.toString(), target);
    filesystem.unmount();
  }
  {
    const { filesystem, mount } = mounted();
    const link = path.join(mount, "src/link.txt");
    await fs.promises.symlink("hello.txt", link);
    const result = await fs.promises.readlink(link, { encoding: "buffer" });
    assert.ok(Buffer.isBuffer(result));
    assert.strictEqual(result.toString(), "hello.txt");
    filesystem.unmount();
  }
  {
    const { filesystem, mount } = mounted();
    const result = await fs.promises.mkdtemp(path.join(mount, "src/tmp-"), {
      encoding: "buffer",
    });
    assert.ok(Buffer.isBuffer(result));
    filesystem.unmount();
  }
  console.log("mounted promise buffer encodings passed");
})();
