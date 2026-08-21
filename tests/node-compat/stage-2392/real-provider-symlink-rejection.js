const assert = require("assert");
const fs = require("fs");
const path = require("path");
const vfs = require("node:vfs");

const root = path.join(process.cwd(), "stage-2392-symlink-reject");
fs.mkdirSync(root, { recursive: true });
const provider = vfs.create(new vfs.RealFSProvider(root));
assert.throws(() => provider.symlinkSync("/etc/passwd", "/escape"), {
  code: "EACCES",
});
