const assert = require("assert");
const { Transform } = require("stream");

let ended = false;
let finished = false;
const transform = new Transform({
  autoDestroy: true,
  transform(data, _encoding, callback) {
    callback(null, data);
  },
  destroy(_error, callback) {
    callback();
  },
});
transform.write("hello");
transform.write("world");
transform.end();
transform.resume();
transform.on("end", () => {
  ended = true;
});
transform.on("finish", () => {
  finished = true;
});
transform.on("close", () => {
  assert.ok(ended);
  assert.ok(finished);
});

console.log("stream auto destroy transform pass");
