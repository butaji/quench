const assert = require("node:assert");
const { internalBinding } = require("internal/test/binding");

const fill = internalBinding("buffer").fill;
const buffer = Buffer.alloc(4);
fill(buffer, 0, 4, 0x61, "utf8");
assert.strictEqual(buffer.toString(), "aaaa");
assert.throws(() => fill(buffer, 1, -1, 0, 1), { code: "ERR_OUT_OF_RANGE" });
assert.throws(() => fill(buffer, 1, 1, -2, 1), { code: "ERR_OUT_OF_RANGE" });
console.log("internal buffer fill passed");
