const fs = require("fs");
const path = `/tmp/quench-node-stage-425-${process.pid}`;
fs.writeFileSync(path, "xyz");

let error;
try {
  fs.createReadStream(path, { start: 2, end: 1 });
} catch (caught) {
  error = caught;
}
if (!error || error.code !== "ERR_OUT_OF_RANGE") {
  throw new Error("read stream must reject an inverted range");
}

fs.unlinkSync(path);
console.log("fs read stream range passed");
