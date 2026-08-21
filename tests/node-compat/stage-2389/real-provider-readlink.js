const assert = require("assert");
const fs = require("fs");
const path = require("path");
const vfs = require("node:vfs");

const root = path.join(process.cwd(), "stage-2389-readlink");
fs.mkdirSync(root, { recursive: true });
fs.writeFileSync(path.join(root, "target.txt"), "x");
const relativeLink = path.join(root, "relative-link");
const absoluteLink = path.join(root, "absolute-link");
fs.rmSync(relativeLink, { force: true });
fs.rmSync(absoluteLink, { force: true });
fs.symlinkSync("target.txt", relativeLink);
fs.symlinkSync(path.join(root, "target.txt"), absoluteLink);
const provider = vfs.create(new vfs.RealFSProvider(root));

assert.strictEqual(provider.readlinkSync("/relative-link"), "target.txt");
assert.strictEqual(provider.readlinkSync("/absolute-link"), "/target.txt");
