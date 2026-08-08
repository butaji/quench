const assert = require("assert");
const fs = require("fs");
const path = require("path");

const source = path.join(
  process.cwd(),
  "tests/node/test/fixtures/copy/kitchen-sink"
);
const destination = path.join(
  process.cwd(),
  "tests/node/test/.tmp.0/cp-existing"
);
fs.mkdirSync(destination, { recursive: true });
assert.rejects(
  fs.promises.cp(source, destination, {
    errorOnExist: true,
    force: false,
    recursive: true
  }),
  { code: "ERR_FS_CP_EEXIST" }
);
