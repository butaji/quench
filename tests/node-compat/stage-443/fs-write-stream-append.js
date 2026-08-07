const fs = require("fs");
const path = `/tmp/quench-node-stage-443-${process.pid}`;
fs.writeFileSync(path, "before");

const stream = fs.createWriteStream(path, { flags: "a" });
stream.on("close", () => {
  if (fs.readFileSync(path, "utf8") !== "beforeafter") {
    throw new Error("append write stream truncated existing data");
  }
  fs.unlinkSync(path);
  console.log("fs write stream append passed");
});
stream.end("after");
