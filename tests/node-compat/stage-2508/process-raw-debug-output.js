const assert = require("assert");
const { spawn } = require("child_process");

if (process.argv[2] === "child") {
  process._rawDebug("I can still %s!", "debug");
} else {
  const child = spawn(process.execPath, [__filename, "child"]);
  let output = "";
  child.stderr.setEncoding("utf8");
  child.stderr.on("data", (chunk) => (output += chunk));
  child.stderr.on(
    "end",
    () => assert.strictEqual(output, "I can still debug!\n"),
  );
  child.on("exit", (code) => assert.strictEqual(code, 0));
}
