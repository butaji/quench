const assert = require("assert");
const vfs = require("node:vfs");

const provider = new vfs.MemoryProvider();
assert.strictEqual(provider.readonly, false);
assert.strictEqual(provider.supportsSymlinks, true);
const filesystem = vfs.create(provider);
filesystem.appendFileSync("/new.txt", "created");
assert.strictEqual(filesystem.readFileSync("/new.txt", "utf8"), "created");
console.log("MemoryProvider surface passed");
