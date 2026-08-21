const assert = require("assert");
const fs = require("fs");
const path = require("path");
const vfs = require("node:vfs");

const root = path.resolve(`/tmp/vfs-real-link-${process.pid}`);
fs.mkdirSync(root, { recursive: true });
fs.writeFileSync(path.join(root, "target.txt"), "ok");
const filesystem = vfs.create(new vfs.RealFSProvider(root));
filesystem.symlinkSync(path.join(root, "target.txt"), "/absolute-link");
assert.strictEqual(filesystem.readlinkSync("/absolute-link"), "/target.txt");
assert.strictEqual(filesystem.realpathSync("/absolute-link"), "/target.txt");
console.log("real provider absolute symlink passed");
