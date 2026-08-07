const fs = require("fs");
const common = require("../common");

const path = "/tmp/quench-node-stage-79-empty";
fs.writeFileSync(path, "");
if (!fs.readFileSync(path) || fs.readFileSync(path, "utf8") !== "") {
  throw new Error("readFile encoding mismatch");
}
fs.readFile(
  path,
  { encoding: "utf8" },
  common.mustCall((error, value) => {
    if (error || value !== "") {
      throw new Error("async readFile encoding mismatch");
    }
  }),
);
