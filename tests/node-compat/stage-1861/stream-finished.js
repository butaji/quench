const { PassThrough, finished } = require("stream");

const stream = new PassThrough();
let callbackCount = 0;
let callbackError;
finished(stream, (error) => {
  callbackError = error;
  callbackCount++;
});
stream.end("done");

setImmediate(() => {
  if (callbackCount !== 0) {
    throw new Error("finished completed before readable");
  }
  stream.resume();
  setImmediate(() => {
    if (callbackCount !== 1) {
      throw new Error("finished callback count mismatch");
    }
    if (callbackError !== undefined) {
      throw new Error(`finished unexpectedly failed: ${callbackError}`);
    }
  });
});
