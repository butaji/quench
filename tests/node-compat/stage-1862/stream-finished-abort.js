const { PassThrough, finished } = require("stream");

const controller = new AbortController();
controller.abort();
const stream = new PassThrough();
let error;
finished(stream, { signal: controller.signal }, (value) => (error = value));

queueMicrotask(() => {
  if (!error || error.name !== "AbortError") {
    throw new Error("finished did not report AbortError");
  }
});
