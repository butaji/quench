const assert = require("assert");
const { Readable, destroy } = require("stream");

const stream = new Readable({ read() {} });
assert.strictEqual(destroy(stream), stream);
assert.strictEqual(stream.destroyed, true);
stream.on("error", (error) => assert.strictEqual(error.name, "AbortError"));
stream.on("close", () => console.log("stream destroy contract passed"));
