const assert = require("assert");
const { Readable } = require("stream");

const readable = new Readable({ read() {} });
readable.destroy();
assert.strictEqual(readable.push(null), false);
assert.strictEqual(readable.readableEnded, false);

console.log("stream push null after destroy pass");
