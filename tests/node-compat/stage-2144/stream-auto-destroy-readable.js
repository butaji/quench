const assert = require("assert");
const { Readable } = require("stream");

let ended = false;
const readable = new Readable({
  autoDestroy: true,
  read() {
    this.push("hello");
    this.push("world");
    this.push(null);
  },
  destroy(_error, callback) {
    callback();
  },
});
readable.resume();
readable.on("end", () => {
  ended = true;
});
readable.on("close", () => assert.ok(ended));

console.log("stream auto destroy readable pass");
