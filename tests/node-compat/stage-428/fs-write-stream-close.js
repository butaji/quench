const fs = require("fs");
const path = `/tmp/quench-node-stage-428-${process.pid}`;
const stream = fs.createWriteStream(path);
stream.on("close", () => {
  if (stream.fd !== null) throw new Error("write stream fd was not closed");
  fs.unlinkSync(path);
  console.log("fs write stream close passed");
});
stream.end("close me");
