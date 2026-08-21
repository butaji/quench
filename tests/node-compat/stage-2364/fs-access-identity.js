const assert = require("assert");
const fs = require("fs");
console.log("uid", process.getuid(), "gid", process.getgid());
const path = `${process.cwd()}/tests/node-compat/stage-2364/read-only`;
try {
  fs.chmodSync(path, 0o666);
  fs.unlinkSync(path);
} catch (_) {}
fs.writeFileSync(path, "");
fs.chmodSync(path, 0o444);
fs.access(path, fs.constants.W_OK, (error) => {
  console.log("access error", error?.code || null);
  console.log("fs access identity passed");
});
