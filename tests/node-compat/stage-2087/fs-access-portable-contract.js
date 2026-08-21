const assert = require("assert");
const fs = require("fs");
const path = require("path");

const file = path.join(
  process.cwd(),
  "tests/node/test/.tmp.0/access-portable.txt",
);
fs.writeFileSync(file, "ok");
fs.accessSync(file, fs.constants.R_OK);
assert.throws(() => fs.accessSync(file, 8), { code: "ERR_OUT_OF_RANGE" });
assert.throws(() => fs.accessSync(file, "r"), { code: "ERR_INVALID_ARG_TYPE" });
assert.throws(() => fs.accessSync(`${file}.missing`), { code: "ENOENT" });

fs.access(file, fs.constants.R_OK, (error) => assert.strictEqual(error, null));
fs.promises.access(file, fs.constants.R_OK).then(() => undefined);
