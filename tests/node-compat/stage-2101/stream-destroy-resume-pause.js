const assert = require("assert");
const { Readable } = require("stream");

const readable = new Readable({ read() {} });
readable.destroy();
readable.on("resume", () => assert.fail("destroyed stream resumed"));
readable.on("pause", () => assert.fail("destroyed stream paused"));
assert.strictEqual(readable.resume(), readable);
assert.strictEqual(readable.pause(), readable);

console.log("destroyed stream resume/pause passed");
