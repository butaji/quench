const assert = require("assert");
const vfs = require("node:vfs");

const filesystem = vfs.create();
filesystem.mkdirSync("/real-dir");
filesystem.writeFileSync("/real-dir/nested.txt", "nested");
filesystem.mkdirSync("/root");
filesystem.symlinkSync("/real-dir", "/root/symdir");
assert.ok(
  filesystem
    .readdirSync("/root", { recursive: true })
    .includes("symdir/nested.txt")
);

const cycle = vfs.create();
cycle.mkdirSync("/dir");
cycle.writeFileSync("/dir/nested.txt", "nested");
cycle.symlinkSync("/dir", "/dir/loop");
assert.deepStrictEqual(cycle.readdirSync("/", { recursive: true }).sort(), [
  "dir",
  "dir/loop",
  "dir/nested.txt"
]);
console.log("recursive symlink readdir passed");
