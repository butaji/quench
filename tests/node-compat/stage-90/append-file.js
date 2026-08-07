const fs = require("fs");
const common = require("../common");
const { Buffer } = require("buffer");

const path = "/tmp/quench-node-stage-90";
fs.writeFileSync(path, "a");
fs.appendFile(
  path,
  Buffer.from("b"),
  common.mustSucceed(() => {
    if (fs.readFileSync(path, "utf8") !== "ab") {
      throw new Error("appendFile mismatch");
    }
  }),
);
