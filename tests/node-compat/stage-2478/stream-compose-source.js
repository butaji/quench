const assert = require("assert");
const { compose, Transform } = require("stream");

const source = (async function* () {
  yield "hello";
  yield "world";
})();
const upper = new Transform({
  transform(chunk, _encoding, callback) {
    callback(null, chunk.toString().toUpperCase());
  }
});

let output = "";
let ended = false;
const keepAlive = setInterval(() => {}, 10);
compose(source, upper)
  .on("data", (chunk) => {
    output += chunk;
  })
  .on("end", () => {
    assert.strictEqual(output, "HELLOWORLD");
    ended = true;
    clearInterval(keepAlive);
  });

process.on("beforeExit", () => {
  assert.strictEqual(ended, true);
});
