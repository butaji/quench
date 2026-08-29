const assert = require("assert");
const { execFile } = require("child_process");

const child = execFile(process.execPath, () => {});
assert.strictEqual(typeof child.kill, "function");
assert.strictEqual(child.kill(), true);
assert.throws(
  () => execFile(process.execPath, [], { signal: "invalid" }, () => {}),
  { code: "ERR_INVALID_ARG_TYPE", name: "TypeError" },
);
