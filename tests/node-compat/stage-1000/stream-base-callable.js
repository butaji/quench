const stream = require("stream");
function Writable() {
  this.writable = true;
  stream.Stream.call(this);
}
Object.setPrototypeOf(Writable.prototype, stream.Stream.prototype);
const writable = new Writable();
if (typeof writable.write !== "function") {
  throw new Error("Stream.write missing");
}
if (writable.write("data") !== true) throw new Error("Stream.write failed");
