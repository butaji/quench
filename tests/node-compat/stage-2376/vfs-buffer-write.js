const assert = require("assert");
const vfs = require("node:vfs");

const fs = vfs.create();
fs.writeFileSync("/buffer.txt", Buffer.from("binary"));
assert.strictEqual(fs.readFileSync("/buffer.txt", "utf8"), "binary");
fs.writeFileSync("/typed.txt", new Uint8Array(Buffer.from("typed")));
assert.strictEqual(fs.readFileSync("/typed.txt", "utf8"), "typed");
console.log("vfs buffer writes passed");
