const fs = require("fs");
const path = `/tmp/quench-node-stage-431-${process.pid}`;
const stream = fs.createWriteStream(path, { autoClose: false });
stream.on("close", () => {
  if (stream.fd === null) {
    throw new Error("write stream ignored autoClose false");
  }
  fs.closeSync(stream.fd);
  fs.unlinkSync(path);
  console.log("fs write stream autoclose passed");
});
stream.end("keep fd");
