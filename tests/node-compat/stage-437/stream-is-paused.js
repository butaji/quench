const { Readable } = require("stream");
const stream = Readable.from(["value"]);

if (stream.isPaused()) throw new Error("new readable should not be paused");
stream.pause();
if (!stream.isPaused()) throw new Error("pause was not observable");
stream.resume();
if (stream.isPaused()) throw new Error("resume was not observable");

console.log("stream isPaused passed");
