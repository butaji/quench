const assert = require("assert");
const vfs = require("node:vfs");

const fs = vfs.create();
fs.writeFileSync("/file.txt", "");
const fd = fs.openSync("/file.txt", "r+");
fs.writeFileSync(fd, "hello");
fs.writeFileSync(fd, new Uint8Array(Buffer.from(" world")));
assert.strictEqual(fs.readFileSync("/file.txt", "utf8"), "hello world");
fs.closeSync(fd);
console.log("vfs descriptor offset passed");
