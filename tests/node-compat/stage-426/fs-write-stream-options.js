const fs = require("fs");
const path = `/tmp/quench-node-stage-426-${process.pid}`;
const stream = fs.createWriteStream(path, { encoding: "utf8" });
stream.end("write stream text");
stream.on("close", () => {
  if (stream.bytesWritten !== 17) {
    throw new Error("bytesWritten was not updated");
  }
  if (fs.readFileSync(path, "utf8") !== "write stream text") {
    throw new Error("write stream encoding was wrong");
  }
  fs.unlinkSync(path);
  console.log("fs write stream options passed");
});
