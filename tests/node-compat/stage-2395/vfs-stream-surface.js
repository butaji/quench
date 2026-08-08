const assert = require("assert");
const vfs = require("node:vfs");

const provider = vfs.create();
provider.writeFileSync("/input.txt", "hello world");
const readable = provider.createReadStream("/input.txt", { encoding: "utf8" });
assert.strictEqual(readable.path, "/input.txt");
const writable = provider.createWriteStream("/output.txt");
assert.strictEqual(writable.path, "/output.txt");
readable.destroy();
writable.destroy();
