const { PassThrough, Readable } = require("stream");

const source = new Readable({ read() {} });
const destination = new PassThrough();
let ended = false;
let received = "";
destination.on("finish", () => (ended = true));
destination.on("data", (chunk) => (received += chunk.toString()));

source.pipe(destination, { end: false });
source.push("payload");
source.push(null);

setImmediate(() => {
  if (received !== "payload") {
    throw new Error("pipe did not forward data");
  }
  if (ended) throw new Error("pipe ignored end:false");
  destination.end();
});
