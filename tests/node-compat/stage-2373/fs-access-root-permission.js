const assert = require("assert");
const fs = require("fs");

const path = `${process.cwd()}/tmp/stage-2373/read-only`;
fs.mkdirSync(`${process.cwd()}/tmp/stage-2373`, { recursive: true });
try {
  fs.chmodSync(path, 0o644);
} catch (error) {
  if (error.code !== "ENOENT") throw error;
}
fs.writeFileSync(path, "");
fs.chmodSync(path, 0o444);

if (process.getuid?.() === 0) {
  assert.doesNotThrow(() => fs.accessSync(path, fs.constants.W_OK));
  fs.access(path, fs.constants.W_OK, (error) => {
    assert.ifError(error);
    console.log("root fs access passed");
  });
}
