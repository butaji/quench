const { Writable } = require("stream");

const expected = new Error("kaboom");
let ticked = false;
const writable = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  },
  destroy(_error, callback) {
    callback(expected);
  },
});

writable.on("error", (error) => {
  if (!ticked) throw new Error("error was synchronous");
  if (error !== expected) throw new Error("wrong replacement error");
  if (!writable._writableState.errorEmitted) {
    throw new Error("error state was not updated");
  }
});
writable.on("close", () => {
  if (!ticked) throw new Error("close was synchronous");
});
writable.destroy();
ticked = true;
