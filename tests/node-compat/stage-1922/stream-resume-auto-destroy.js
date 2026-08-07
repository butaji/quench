const assert = require("assert");
const { Readable } = require("stream");

const events = [];
const readable = new Readable({
  autoDestroy: true,
  read() {
    this.push("hello");
    this.push("world");
    this.push(null);
  },
  destroy(error, callback) {
    events.push("destroy");
    callback(error);
  },
});
readable.resume();
readable.on("end", () => events.push("end"));
readable.on("close", () => {
  assert.deepStrictEqual(events, ["end", "destroy"]);
  console.log("stream resume auto-destroy passed");
});
