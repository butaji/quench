const { Readable } = require("stream");
const readable = new Readable();
let errorCode;
readable.on("error", (error) => {
  errorCode = error.code;
});
if (readable.push({ invalid: true })) {
  throw new Error("object chunk was accepted");
}
if (errorCode !== "ERR_INVALID_ARG_TYPE") {
  throw new Error("chunk error was missing");
}
