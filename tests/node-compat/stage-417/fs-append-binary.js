const fs = require("fs");
const path = `/tmp/quench-node-stage-417-${process.pid}`;
const bytes = Buffer.from([0xff, 0x00, 0xc3, 0x28]);

fs.appendFileSync(path, bytes);
const result = fs.readFileSync(path);
if (!result.equals(bytes)) {
  throw new Error("binary append bytes were corrupted");
}
fs.unlinkSync(path);

console.log("fs append binary passed");
