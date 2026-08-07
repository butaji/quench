const { Readable, Writable } = require("stream");

const readable = new Readable({ objectMode: true });
const writable = new Writable({ objectMode: true });
if (readable.readableObjectMode !== true) {
  throw new Error("readable object mode was not exposed");
}
if (writable.writableObjectMode !== true) {
  throw new Error("writable object mode was not exposed");
}
if (new Readable().readableObjectMode !== false) {
  throw new Error("readable byte mode was wrong");
}
if (new Writable().writableObjectMode !== false) {
  throw new Error("writable byte mode was wrong");
}

console.log("stream object mode passed");
