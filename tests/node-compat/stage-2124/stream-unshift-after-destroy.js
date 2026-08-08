const assert = require("assert");
const { Readable } = require("stream");

const readable = new Readable({ read() {} });
readable.destroy();
assert.strictEqual(readable.unshift("ignored"), false);

console.log("stream unshift after destroy pass");
