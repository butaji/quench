const { Readable } = require("stream");
const stream = new Readable();
const chunks = [];
stream.on("data", (chunk) => chunks.push(chunk));

stream.push("first");
stream.unshift("before");
if (chunks.join("") !== "firstbefore") {
  throw new Error("unshift did not deliver its chunk");
}

console.log("stream unshift passed");
