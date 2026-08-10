const { Writable } = require("stream");

const first = new Error("kaboom 1");
const writable = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  },
});
let errors = 0;
let ticked = false;

writable.on("error", (error) => {
  errors++;
  if (!ticked) throw new Error("error was synchronous");
  if (error !== first) throw new Error("first destroy error was not preserved");
  if (!writable._writableState.errorEmitted) {
    throw new Error("error state was not emitted");
  }
});
writable.on("close", () => {
  if (errors !== 1) throw new Error(`expected one error, got ${errors}`);
});

writable.destroy(first);
writable.destroy(new Error("kaboom 2"));
if (writable._writableState.errored !== first) {
  throw new Error("destroy replaced the first error");
}
ticked = true;
