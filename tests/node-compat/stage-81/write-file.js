const fs = require("fs");
const common = require("../common");
const { Buffer } = require("buffer");

const path = "/tmp/quench-node-stage-81";
const data = Buffer.from([0, 1, 255]);
fs.writeFile(
  path,
  data,
  common.mustSucceed(() => {
    const result = fs.readFileSync(path);
    if (!Buffer.isBuffer(result) || result.length !== data.length) {
      throw new Error("writeFile Buffer mismatch");
    }
  }),
);
