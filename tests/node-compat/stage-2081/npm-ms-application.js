const assert = require("assert");
const ms = require("ms");

assert.strictEqual(ms("2 days"), 172800000);
assert.strictEqual(ms("1h 30m"), undefined);
assert.strictEqual(ms(60000), "1m");
assert.strictEqual(ms(0), "0ms");
