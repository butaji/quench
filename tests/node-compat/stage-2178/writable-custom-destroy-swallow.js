const { Writable } = require("stream");

const expected = new Error("kaboom");
const writable = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  },
  destroy(error, callback) {
    if (error !== expected) throw new Error("wrong destroy error");
    callback();
  },
});

writable.on("error", () => {
  throw new Error("destroy error was not swallowed");
});
writable.on("close", () => {
  if (!writable.destroyed) throw new Error("writable was not destroyed");
});
writable.destroy(expected);
