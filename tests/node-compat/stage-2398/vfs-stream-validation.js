const assert = require("assert");
const vfs = require("node:vfs");

const provider = vfs.create();
provider.writeFileSync("/file.txt", "abc");
assert.throws(
  () => provider.createReadStream("/file.txt", { start: 2, end: 1 }),
  { code: "ERR_OUT_OF_RANGE" },
);
assert.throws(() => provider.createReadStream("/file.txt", { start: -1 }), {
  code: "ERR_OUT_OF_RANGE",
});
