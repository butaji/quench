const fs = require("fs");
const common = require("../common");
const path = require("path");

const root = "/tmp/quench-node-stage-68";
fs.mkdirSync(root, { recursive: true });
const nested = path.join(root, "a", "b");
fs.mkdir(
  nested,
  { recursive: true },
  common.mustCall((error) => {
    if (error) throw error;
    if (!fs.existsSync(nested)) {
      throw new Error("recursive mkdir did not create directory");
    }
  }),
);
fs.mkdirSync(path.join(root, "sync"), { recursive: true });
