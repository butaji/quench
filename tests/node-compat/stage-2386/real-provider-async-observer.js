const assert = require("assert");
const fs = require("fs");
const path = require("path");
const vfs = require("node:vfs");

const root = path.join(process.cwd(), "stage-2386-real-observer");
fs.mkdirSync(root, { recursive: true });
fs.writeFileSync(path.join(root, "file.txt"), "async content");
const provider = vfs.create(new vfs.RealFSProvider(root));

const fd = provider.openSync("/file.txt", "r");
const handle = globalThis.__quenchVfsFdHandles.get(fd).entry;
assert.strictEqual(typeof handle.readFile, "function");
assert.strictEqual(handle.readFileSync("utf8"), "async content");
provider.closeSync(fd);
