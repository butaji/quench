const assert = require("assert");
const fs = require("fs");
const path = require("path");
const vfs = require("node:vfs");

const root = path.join(process.cwd(), "stage-2388-realpath");
fs.mkdirSync(root, { recursive: true });
fs.writeFileSync(path.join(root, "file.txt"), "content");
const provider = vfs.create(new vfs.RealFSProvider(root));

assert.strictEqual(provider.realpathSync("/file.txt"), "/file.txt");
assert.strictEqual(
  provider.realpathSync("/file.txt", { encoding: "buffer" }).toString(),
  "/file.txt"
);
