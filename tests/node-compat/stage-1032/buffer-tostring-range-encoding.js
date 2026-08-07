const assert = require("assert");
const { Buffer } = require("buffer");
const buffer = Buffer.from("abc");

for (const encoding of [0, null]) {
  assert.throws(() => buffer.toString(encoding, 1, 2), {
    code: "ERR_UNKNOWN_ENCODING",
    name: "TypeError",
    message: `Unknown encoding: ${encoding}`,
  });
}

assert.strictEqual(buffer.toString({ toString: () => "ascii" }, 0, 3), "abc");
