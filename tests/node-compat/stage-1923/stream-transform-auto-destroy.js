const assert = require("assert");
const { Transform } = require("stream");

const events = [];
const transform = new Transform({
  autoDestroy: true,
  transform(data, encoding, callback) {
    callback(null, data);
  },
  destroy(error, callback) {
    events.push("destroy");
    callback(error);
  },
});
transform.write("hello");
transform.write("world");
transform.end();
transform.resume();
transform.on("end", () => events.push("end"));
transform.on("finish", () => events.push("finish"));
transform.on("close", () => {
  assert.ok(events.includes("end"));
  assert.ok(events.includes("finish"));
  assert.ok(events.includes("destroy"));
  console.log("stream transform auto-destroy passed");
});
