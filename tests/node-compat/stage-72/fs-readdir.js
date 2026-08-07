const fs = require("fs");
const common = require("../common");

const root = "/tmp/quench-node-stage-72";
fs.mkdirSync(root, { recursive: true });
const file = `${root}/entry`;
fs.closeSync(fs.openSync(file, "w"));
if (!fs.readdirSync(root).includes("entry")) {
  throw new Error("directory entry missing");
}
fs.readdir(
  root,
  common.mustSucceed((entries) => {
    if (!entries.includes("entry")) {
      throw new Error("async directory entry missing");
    }
  }),
);
fs.readdir(
  file,
  common.mustCall((error) => {
    if (!error || error.code !== "ENOTDIR") throw new Error("expected ENOTDIR");
  }),
);
