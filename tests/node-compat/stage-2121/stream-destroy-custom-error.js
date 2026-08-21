const assert = require("assert");
const { Readable } = require("stream");

const expected = new Error("kaboom");
let destroyError;
const readable = new Readable({
  read() {},
  destroy(error, callback) {
    destroyError = error;
    callback(error);
  },
});
readable.on("error", (error) => assert.strictEqual(error, expected));
readable.on("close", () => assert.strictEqual(readable.destroyed, true));
readable.destroy(expected);
assert.strictEqual(destroyError, expected);

console.log("stream custom destroy error pass");
