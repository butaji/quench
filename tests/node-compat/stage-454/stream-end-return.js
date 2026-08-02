const { Writable } = require("stream");

const stream = new Writable();
let finished = false;
stream.on("finish", () => (finished = true));

if (stream.end() !== stream) throw new Error("end was not chainable");
setTimeout(() => {
  if (!finished) throw new Error("finish was not emitted");
  console.log("stream end return passed");
}, 0);
