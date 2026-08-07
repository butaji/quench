const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.rmSync("missing-rm-target", { recursive: true }), {
  code: "ENOENT",
});
assert.doesNotThrow(() =>
  fs.rmSync("missing-rm-target", { recursive: true, force: true })
);

console.log("rm missing target passed");
