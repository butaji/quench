const fs = require("fs");
const path = `/tmp/quench-node-stage-147-${process.pid}`;
fs.writeFileSync(path, "x");
if (fs.realpathSync.native(path) !== fs.realpathSync(path))
  throw new Error("native realpath mismatch");
