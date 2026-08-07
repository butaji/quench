const { Writable } = require("stream");

const stream = new Writable({ highWaterMark: 4 });
if (stream.write("😀") !== false) {
  throw new Error("multibyte write did not reach the high water mark");
}
if (stream.writableLength !== 4) {
  throw new Error(`writableLength was ${stream.writableLength}, expected 4`);
}

console.log("stream write byte length passed");
