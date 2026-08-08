const assert = require("assert");
const vfs = require("node:vfs");

(async () => {
  const filesystem = vfs.create();
  filesystem.writeFileSync("/pending.txt", "a");
  const iterator = filesystem.promises.watch("/pending.txt", { interval: 25 });
  const pending = iterator.next();
  queueMicrotask(() => iterator.return());
  assert.deepStrictEqual(await pending, { done: true, value: undefined });
  console.log("watch promises pending close passed");
})();
