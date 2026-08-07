const fs = require("fs");
const assert = require("assert");
const common = require("../common");

const target = "/tmp/quench-node-stage-107-target";
const link = "/tmp/quench-node-stage-107-link";
try {
  fs.unlinkSync(link);
} catch (_) {}
fs.writeFileSync(target, "link");
fs.symlink(
  target,
  link,
  "file",
  common.mustSucceed(() => {
    fs.readlink(
      link,
      common.mustSucceed((value) => {
        assert.strictEqual(value, target);
        fs.unlinkSync(link);
        fs.rmSync(target);
      }),
    );
  }),
);
