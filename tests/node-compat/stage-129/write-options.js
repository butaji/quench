const fs = require("fs");
const path = `/tmp/quench-node-stage-129-${process.pid}`;
fs.writeFileSync(path, "hello ", { encoding: "utf8", flag: "a" });
fs.writeFileSync(path, "world!", { encoding: "utf8", flag: "a" });
if (fs.readFileSync(path, "utf8") !== "hello world!")
  throw new Error("append option mismatch");
fs.writeFileSync(path, Buffer.from("4142", "hex"), { encoding: "hex" });
if (fs.readFileSync(path, "utf8") !== "AB")
  throw new Error("encoding option mismatch");
