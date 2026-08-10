const assert = require("assert");
const fs = require("fs");
const path = require("path");

const root = path.join(
  process.cwd(),
  "tests/node/test/.tmp.0/cp-incompatible-options",
);
fs.mkdirSync(root, { recursive: true });
fs.writeFileSync(path.join(root, "source"), "source");
assert.throws(
  () =>
    fs.cpSync(path.join(root, "source"), path.join(root, "dest"), {
      dereference: true,
      verbatimSymlinks: true,
    }),
  { code: "ERR_INCOMPATIBLE_OPTION_PAIR" },
);
