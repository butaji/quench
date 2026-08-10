const assert = require("assert");
const { Readable } = require("stream");

const readable = new Readable({
  read() {
    destination.emit("error", new Error("fail"));
  },
});
const destination = new Readable({
  autoDestroy: true,
  destroy(_error, callback) {
    callback();
  },
});
readable.pipe(destination);

setTimeout(() => {
  assert.strictEqual(destination.destroyed, true);
  console.log("stream auto destroy pipe readable error pass");
}, 0);
