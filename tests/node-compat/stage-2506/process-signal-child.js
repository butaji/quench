const assert = require("assert");
const { spawn } = require("child_process");

if (process.argv[2] === "--do-test") {
  process.on("SIGINT", () => {
    process.removeAllListeners("SIGINT");
    process.kill(process.pid, "SIGINT");
  });
  process.stdin.resume();
  process.kill(process.pid, "SIGINT");
} else {
  const child = spawn(process.execPath, [__filename, "--do-test"]);
  child.once("exit", (code, signal) => {
    assert.strictEqual(code, null);
    assert.strictEqual(signal, "SIGINT");
  });
}
