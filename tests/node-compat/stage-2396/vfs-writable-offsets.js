const assert = require("assert");
const vfs = require("node:vfs");

const provider = vfs.create();
const stream = provider.createWriteStream("/chunks.txt");
assert.strictEqual(stream.path, "/chunks.txt");
stream.write("hello");
stream.end(" world");
assert.strictEqual(typeof stream.write, "function");
