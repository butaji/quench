const assert = require("assert");
const { Transform } = require("stream");

const events = [];
const transform = new Transform({
  autoDestroy: true,
  transform(chunk, _encoding, callback) {
    callback(null, chunk);
  },
  destroy(error, callback) {
    events.push("destroy");
    callback(error);
  },
});

transform.end("discarded output");
transform.resume();
transform.on("end", () => events.push("end"));
transform.on("finish", () => events.push("finish"));
transform.on("close", () => {
  events.push("close");
  assert.deepStrictEqual(events, ["end", "finish", "destroy", "close"]);
});
