const { Readable } = require("stream");

const stream = new Readable();
if (stream.readableFlowing !== null) {
  throw new Error("initial flowing state was not null");
}

stream.on("data", () => {});
if (stream.readableFlowing !== true) {
  throw new Error("data listener did not enable flowing");
}
stream.pause();
if (stream.readableFlowing !== false) {
  throw new Error("pause did not disable flowing");
}
stream.resume();
if (stream.readableFlowing !== true) {
  throw new Error("resume did not enable flowing");
}

console.log("stream flowing state passed");
