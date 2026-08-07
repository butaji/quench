const assert = require("assert");
const { Transform } = require("stream");

const transform = new Transform({
  transform(chunk, encoding, callback) {
    callback(null, String(chunk).toUpperCase());
  },
});
let direct = "";
transform.on("data", (chunk) => (direct += chunk));
transform.end("ab");

setTimeout(() => {
  assert.strictEqual(direct, "AB");
}, 10);
