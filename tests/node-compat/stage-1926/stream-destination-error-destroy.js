const assert = require("assert");
const { Readable, Writable } = require("stream");

for (
  const destination of [
    new Readable({
      autoDestroy: true,
      destroy(error, callback) {
        assert.strictEqual(error, null);
        callback();
      },
    }),
    new Writable({
      autoDestroy: true,
      destroy(error, callback) {
        assert.strictEqual(error, null);
        callback();
      },
    }),
  ]
) {
  destination.on("error", () => {});
  destination.on("close", () => console.log("destination closed"));
  destination.emit("error", new Error("fail"));
}
setTimeout(() => console.log("stream destination error destroy passed"), 0);
