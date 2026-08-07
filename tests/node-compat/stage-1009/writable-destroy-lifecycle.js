const { Writable } = require("stream");
const events = [];
const expected = new Error("destroyed");
const writable = new Writable({
  destroy(error, callback) {
    if (error !== expected) throw new Error("destroy error mismatch");
    events.push("destroy");
    callback();
  },
});
writable.on("error", () => events.push("error"));
writable.on("close", () => {
  events.push("close");
  if (events.join(",") !== "destroy,close") {
    throw new Error(`destroy ordering mismatch: ${events.join(",")}`);
  }
});
writable.destroy(expected);
if (!writable.destroyed || writable._writableState.errored !== expected) {
  throw new Error("destroy state mismatch");
}
