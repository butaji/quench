const assert = require("assert");
const fs = require("fs");
const path = require("path");
const vfs = require("node:vfs");

const root = path.join(process.cwd(), "stage-2390-watch");
fs.mkdirSync(root, { recursive: true });
const provider = vfs.create(new vfs.RealFSProvider(root));
fs.writeFileSync(path.join(root, "file.txt"), "a");

const watcher = provider.watch("/file.txt", { persistent: false });
assert.strictEqual(typeof watcher.close, "function");
watcher.close();
const listener = () => {};
provider.watchFile("/file.txt", { persistent: false }, listener);
provider.unwatchFile("/file.txt", listener);
const iterator = provider.promises.watch("/file.txt", { persistent: false });
assert.strictEqual(typeof iterator.return, "function");
