const assert = require("assert");
const { Writable, PassThrough } = require("stream");

const writable = new Writable();
assert.strictEqual(writable.closed, false);
writable.destroy();
writable.on("close", () => assert.strictEqual(writable.closed, true));

const pass = new PassThrough();
assert.strictEqual(pass.closed, false);
pass.end();
setImmediate(() => assert.strictEqual(pass.closed, true));
