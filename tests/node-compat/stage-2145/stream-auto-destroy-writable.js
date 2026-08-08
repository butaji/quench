const assert = require("assert");
const { Writable } = require("stream");

let finished = false;
const writable = new Writable({
  autoDestroy: true,
  write(_data, _encoding, callback) {
    callback();
  },
  destroy(_error, callback) {
    callback();
  }
});
writable.write("hello");
writable.write("world");
writable.end();
writable.on("finish", () => {
  finished = true;
});
writable.on("close", () => assert.ok(finished));

console.log("stream auto destroy writable pass");
