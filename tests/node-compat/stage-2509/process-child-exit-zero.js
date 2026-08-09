const assert = require("assert");
const { spawn } = require("child_process");

if (process.argv[2] === "child") {
  process.exit(0);
} else {
  const child = spawn(process.execPath, [__filename, "child"]);
  child.once("close", (code, signal) => {
    assert.strictEqual(code, 0);
    assert.strictEqual(signal, null);
  });
}
