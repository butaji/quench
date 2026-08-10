const { Writable } = require("stream");

let ticked = false;
const writable = new Writable({
  destroy(_error, callback) {
    process.nextTick(callback, new Error("kaboom 1"));
  },
  write(_chunk, _encoding, callback) {
    callback();
  },
});

writable.on("error", (error) => {
  if (!ticked) throw new Error("error was synchronous");
  if (error.message !== "kaboom 1") throw new Error("wrong destroy error");
  if (!writable._writableState.errorEmitted) {
    throw new Error("error state was not emitted");
  }
});
writable.on("close", () => {
  if (!ticked) throw new Error("close was synchronous");
  writable.on("error", () => {
    throw new Error("error emitted twice");
  });
});

writable.destroy();
writable.destroy(new Error("kaboom 2"));
if (writable._writableState.errored !== null) {
  throw new Error("pending destroy replaced the state");
}
ticked = true;
