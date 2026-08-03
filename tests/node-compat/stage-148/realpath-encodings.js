const fs = require("fs");
const path = `/tmp/quench-node-stage-148-${process.pid}`;
fs.writeFileSync(path, "x");
const expected = fs.realpathSync(path);
if (fs.realpathSync(path, "utf8") !== expected)
  throw new Error("utf8 realpath mismatch");
if (fs.realpathSync(path, "buffer").toString() !== expected)
  throw new Error("buffer realpath mismatch");
