const { PassThrough, finished } = require("stream");

const controller = new AbortController();
const stream = new PassThrough();
let abortError;
finished(
  stream,
  { signal: controller.signal },
  (error) => (abortError = error),
);
controller.abort();

const writable = new PassThrough();
let completed = false;
finished(
  writable,
  { readable: false, writable: true },
  () => (completed = true),
);
writable.end("ok");

setImmediate(() => {
  if (!abortError || abortError.name !== "AbortError") {
    throw new Error("future abort was not reported");
  }
  if (!completed) throw new Error("writable-only finished did not complete");
});
