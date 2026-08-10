const assert = require("assert");
const vfs = require("node:vfs");

(async () => {
  const filesystem = vfs.create();
  filesystem.writeFileSync("/file.txt", "original");
  assert.strictEqual(
    filesystem.readFileSync("/file.txt", { flag: "w+" }).length,
    0,
  );
  assert.strictEqual(
    filesystem.readFileSync("/new.txt", { flag: "a+" }).length,
    0,
  );
  assert.throws(
    () => filesystem.readFileSync("/file.txt", { encoding: "bogus" }),
    { code: "ERR_UNKNOWN_ENCODING" },
  );
  filesystem.mkdirSync("/dir");
  assert.throws(() => filesystem.rmSync("/dir"), { code: "EISDIR" });
  await assert.rejects(filesystem.promises.rm("/dir"), { code: "EISDIR" });
  console.log("read and rm contracts passed");
})();
