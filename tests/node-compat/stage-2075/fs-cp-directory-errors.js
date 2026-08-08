const assert = require("assert");
const fs = require("fs");
const path = require("path");

const root = path.join(
  process.cwd(),
  "tests/node/test/.tmp.0/cp-directory-errors"
);
fs.rmSync(root, { recursive: true, force: true });
fs.mkdirSync(path.join(root, "source"), { recursive: true });
fs.writeFileSync(path.join(root, "source", "file.txt"), "file");
fs.writeFileSync(path.join(root, "destination.txt"), "destination");

assert.throws(
  () =>
    fs.cpSync(path.join(root, "source"), path.join(root, "destination.txt")),
  (error) => error.code === "ERR_FS_CP_DIR_TO_NON_DIR"
);

const source = path.join(root, "source");
fs.symlinkSync(path.join(source, "file.txt"), path.join(source, "link.txt"));
fs.mkdirSync(path.join(root, "destination"));
fs.writeFileSync(path.join(root, "destination", "link.txt"), "existing");
assert.throws(
  () => fs.cpSync(source, path.join(root, "destination"), { recursive: true }),
  (error) => error.code === "EEXIST"
);
