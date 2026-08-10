const assert = require("assert");
const tty = require("tty");
assert.strictEqual(typeof tty.WriteStream, "function");
const stream = new tty.WriteStream(1);
assert.strictEqual(typeof stream, "object");
assert.strictEqual(typeof stream.isTTY, "boolean");
console.log("tty style probe passed");
