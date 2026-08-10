const assert = require("assert");
const fs = require("fs");
const path = require("path");

const root = path.join(
  process.cwd(),
  "tests/node/test/.tmp.0/cp-symlink-options",
);
fs.rmSync(root, { recursive: true, force: true });
fs.mkdirSync(path.join(root, "source"), { recursive: true });
fs.writeFileSync(path.join(root, "source", "target.txt"), "target");
fs.symlinkSync(
  path.join(root, "source", "target.txt"),
  path.join(root, "source", "link.txt"),
);

fs.cpSync(path.join(root, "source", "link.txt"), path.join(root, "copy.txt"), {
  dereference: true,
});
assert.strictEqual(fs.lstatSync(path.join(root, "copy.txt")).isFile(), true);

fs.cpSync(
  path.join(root, "source", "link.txt"),
  path.join(root, "link-copy.txt"),
  {
    dereference: false,
  },
);
assert.strictEqual(
  fs.lstatSync(path.join(root, "link-copy.txt")).isSymbolicLink(),
  true,
);

fs.mkdirSync(path.join(root, "existing-dir"));
assert.throws(
  () =>
    fs.cpSync(path.join(root, "source"), path.join(root, "existing-dir"), {
      recursive: true,
      errorOnExist: true,
    }),
  (error) => error.code === "ERR_FS_CP_EEXIST",
);

fs.cp(
  path.join(root, "source", "link.txt"),
  path.join(root, "async-copy.txt"),
  { dereference: true },
  (error) => {
    assert.strictEqual(error, null);
    assert.strictEqual(
      fs.lstatSync(path.join(root, "async-copy.txt")).isFile(),
      true,
    );
  },
);
