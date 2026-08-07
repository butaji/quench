const fs = require("fs");
const common = require("../common");

const root = "/tmp/quench-node-stage-88";
const first = `${root}-first`;
const second = `${root}-second`;
fs.mkdirSync(root, { recursive: true });
fs.mkdirSync(first);
try {
  fs.mkdirSync(first);
  throw new Error("duplicate mkdir accepted");
} catch (error) {
  if (error.code !== "EEXIST") throw error;
}
fs.rename(
  first,
  second,
  common.mustSucceed(() => fs.rmdir(second, common.mustSucceed())),
);
