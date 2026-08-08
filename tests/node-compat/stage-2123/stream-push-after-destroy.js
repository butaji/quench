const assert = require("assert");
const { Readable } = require("stream");

const readable = new Readable({ read() {} });
readable.destroy();
assert.strictEqual(readable.push("ignored"), false);
assert.strictEqual(readable.destroyed, true);

console.log("stream push after destroy pass");
