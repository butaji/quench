const assert = require("assert");
const fs = require("fs");
const path = require("path");
const vfs = require("node:vfs");
const { getVirtualFd } = require("internal/vfs/fd");

const root = path.join(process.cwd(), "stage-2383-real-handle");
fs.mkdirSync(root, { recursive: true });
fs.writeFileSync(path.join(root, "file.txt"), "hello world");

const provider = vfs.create(new vfs.RealFSProvider(root));
const fd = provider.openSync("/file.txt", "r+");
const handle = getVirtualFd(fd).entry;
const buffer = Buffer.alloc(5);

assert.strictEqual(handle.readSync(buffer, 0, 5, 0), 5);
assert.strictEqual(buffer.toString(), "hello");
assert.strictEqual(handle.writeSync(Buffer.from("J"), 0, 1, 0), 1);
assert.strictEqual(handle.readFileSync("utf8"), "Jello world");
provider.closeSync(fd);
