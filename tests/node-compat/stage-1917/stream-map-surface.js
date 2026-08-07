const assert = require("assert");
const { Readable } = require("stream");

const source = new Readable({
  read() {
    this.push(1);
    this.push(2);
    this.push(null);
  },
});
assert.strictEqual(typeof source.map, "function");
const mapped = source.map((value) => value * 2);
assert.strictEqual(typeof mapped[Symbol.asyncIterator], "function");
console.log("stream map surface passed");
