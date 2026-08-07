const { Readable } = require("stream");
if (!Object.hasOwn(Readable.prototype, "readableEnded")) {
  throw new Error("Readable.prototype.readableEnded is missing");
}
if (new Readable().readableEnded !== false) {
  throw new Error("readableEnded did not default to false");
}
