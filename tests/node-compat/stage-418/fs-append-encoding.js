const fs = require("fs");
const path = `/tmp/quench-node-stage-418-${process.pid}`;

fs.appendFileSync(path, "ff", { encoding: "hex" });
fs.appendFileSync(path, "IA==", { encoding: "base64" });
const result = fs.readFileSync(path);
if (!result.equals(Buffer.from([0xff, 0x20]))) {
  throw new Error("appendFileSync ignored string encoding");
}
fs.unlinkSync(path);

console.log("fs append encoding passed");
