const { Writable } = require("stream");
const stream = new Writable();
const chunks = [];
stream.on("data", (chunk) => chunks.push(chunk));

stream.cork();
stream.write("a");
stream.write("b");
if (chunks.length !== 0) throw new Error("corked writes were emitted early");
stream.uncork();
if (chunks.join("") !== "ab") throw new Error("uncork lost buffered writes");

console.log("stream cork buffer passed");
