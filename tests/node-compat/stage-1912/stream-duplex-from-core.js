const assert = require("assert");
const { Duplex, Readable, Writable } = require("stream");

assert.strictEqual(typeof Duplex, "function");
assert.strictEqual(typeof Duplex.from, "function");

const readable = Duplex.from(
  new Readable({
    read() {
      this.push("readable");
      this.push(null);
    },
  }),
);
assert.strictEqual(readable.readable, true);
assert.strictEqual(readable.writable, false);

const writableValues = [];
const writable = Duplex.from(
  new Writable({
    write(chunk, encoding, callback) {
      writableValues.push(chunk.toString());
      callback();
    },
  }),
);
assert.strictEqual(writable.readable, false);
assert.strictEqual(writable.writable, true);
writable.end("writable");

const pair = Duplex.from({ readable: Readable.from(["pair"]), writable });
assert.strictEqual(pair.readable, true);
assert.strictEqual(pair.writable, true);

const iterable = Duplex.from(["iterable"]);
assert.strictEqual(iterable.readable, true);
assert.strictEqual(iterable.writable, false);

const promised = Duplex.from(Promise.resolve("promise"));
assert.strictEqual(promised.readable, true);
assert.strictEqual(promised.writable, false);

Promise.all([
  new Promise((resolve) => {
    readable.once("data", (value) => {
      assert.strictEqual(value.toString(), "readable");
    });
    readable.once("end", resolve);
  }),
  new Promise((resolve) => {
    writable.once("finish", () => {
      assert.deepStrictEqual(writableValues, ["writable"]);
      resolve();
    });
  }),
  new Promise((resolve) => {
    pair.once("data", (value) => assert.strictEqual(value, "pair"));
    pair.once("end", resolve);
  }),
  new Promise((resolve) => {
    iterable.once("data", (value) => assert.strictEqual(value, "iterable"));
    iterable.once("end", resolve);
  }),
  new Promise((resolve) => {
    promised.once("data", (value) => assert.strictEqual(value, "promise"));
    promised.once("end", resolve);
  }),
]).then(() => console.log("stream Duplex.from core passed"));
