const assert = require("assert");
const { textSync } = require("stream/iter");

const source = {
  *[Symbol.iterator]() {
    yield [Buffer.from("hello")];
  },
};
assert.strictEqual(textSync(source), "hello");
console.log("stream iter sync adapter passed");
