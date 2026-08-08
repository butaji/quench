const assert = require("assert");
const { once } = require("events");
const vfs = require("node:vfs");

(async () => {
  const filesystem = vfs.create();
  filesystem.writeFileSync("/file.txt", "a");
  const watcher = filesystem.watch("/file.txt", {
    interval: 5,
    encoding: "buffer"
  });
  const changed = once(watcher, "change");
  filesystem.writeFileSync("/file.txt", "changed");
  const [eventType, filename] = await changed;
  assert.strictEqual(eventType, "change");
  assert.deepStrictEqual(filename, Buffer.from("file.txt"));
  watcher.close();

  const directory = vfs.create();
  directory.mkdirSync("/dir");
  const directoryWatcher = directory.watch("/dir", { interval: 5 });
  const created = once(directoryWatcher, "change");
  directory.writeFileSync("/dir/new.txt", "x");
  assert.deepStrictEqual(await created, ["rename", "new.txt"]);
  directoryWatcher.close();
  console.log("VFS watch passed");
})();
