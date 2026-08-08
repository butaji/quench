const assert = require("assert");
const fs = require("fs");
const path = require("path");
const vfs = require("node:vfs");

const root = path.join(process.cwd(), "stage-2389-readlink");
fs.mkdirSync(root, { recursive: true });
fs.writeFileSync(path.join(root, "target.txt"), "x");
fs.symlinkSync("target.txt", path.join(root, "relative-link"));
fs.symlinkSync(path.join(root, "target.txt"), path.join(root, "absolute-link"));
const provider = vfs.create(new vfs.RealFSProvider(root));

assert.strictEqual(provider.readlinkSync("/relative-link"), "target.txt");
assert.strictEqual(provider.readlinkSync("/absolute-link"), "/target.txt");
