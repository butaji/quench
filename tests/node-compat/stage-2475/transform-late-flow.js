const assert = require("assert");
const { compose, Transform } = require("stream");

const duplicate = new Transform({
  transform(chunk, _encoding, callback) {
    callback(null, chunk.toString() + chunk.toString());
  },
});
const upper = new Transform({
  transform(chunk, _encoding, callback) {
    callback(null, chunk.toString().toUpperCase());
  },
});

let output = "";
let ended = false;
compose(duplicate, upper)
  .end("asd")
  .on("data", (chunk) => {
    output += chunk;
  })
  .on("end", () => {
    assert.strictEqual(output, "ASDASD");
    ended = true;
  });

process.on("beforeExit", () => {
  assert.strictEqual(ended, true);
});
