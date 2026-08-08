const assert = require("assert");
const vfs = require("node:vfs");

(async () => {
  const filesystem = vfs.create();
  filesystem.mkdirSync("/real");
  filesystem.symlinkSync("/real", "/link");
  filesystem.mkdirSync("/link/subdir/deep", { recursive: true });
  assert.strictEqual(filesystem.existsSync("/real/subdir/deep"), true);

  filesystem.writeFileSync("/file.txt", "a");
  const iter = filesystem.promises.watch("/file.txt", { interval: 5 });
  filesystem.writeFileSync("/file.txt", "b");
  const event = await iter.next();
  assert.strictEqual(event.value.eventType, "change");
  assert.deepStrictEqual(await iter.throw(new Error("close")), {
    done: true,
    value: undefined
  });
  console.log("watch promises and mkdir symlink passed");
})();
