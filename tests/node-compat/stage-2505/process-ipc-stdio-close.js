const assert = require("assert");
const { spawn } = require("child_process");

if (process.argv[2] === "child") {
  process.on("message", () => process.disconnect());
} else {
  const child = spawn(process.execPath, [__filename, "child"], {
    stdio: ["pipe", "pipe", "pipe", "ipc"],
  });
  child.on("close", (code, signal) => {
    assert.strictEqual(code, 0);
    assert.strictEqual(signal, null);
  });
  child.stdout.destroy();
  child.send("go");
}
