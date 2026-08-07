const fs = require("fs");
const common = require("../common");

const root = "/tmp/quench-node-stage-69";
fs.mkdirSync(root, { recursive: true });
const file = `${root}/entry`;
const fd = fs.openSync(file, "w");
fs.closeSync(fd);
if (!fs.readdirSync(root).includes("entry")) {
  throw new Error("readdirSync missed entry");
}
fs.readdir(
  root,
  common.mustCall((error, entries) => {
    if (error) throw error;
    if (!entries.includes("entry")) throw new Error("readdir missed entry");
  }),
);
