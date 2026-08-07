const fs = require("fs");
const path = `/tmp/quench-node-stage-422-${process.pid}`;
const stream = fs.createWriteStream(path);
let opened = false;
stream.on("open", () => {
  opened = true;
});
stream.on("close", () => {
  if (!opened) throw new Error("write stream closed before open");
  if (fs.readFileSync(path, "utf8") !== "stream data") {
    throw new Error("write stream content was incorrect");
  }
  fs.unlinkSync(path);
  console.log("fs write stream end passed");
});
stream.end("stream data", "utf8");
