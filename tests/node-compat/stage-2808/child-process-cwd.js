const assert = require("assert");
const { spawn } = require("child_process");

const child = spawn("pwd", { cwd: "/tmp" });
assert.strictEqual(typeof child.pid, "number");
child.stdout.setEncoding("utf8");
let output = "";
child.stdout.on("data", (chunk) => (output += chunk));
child.on("close", () => assert.strictEqual(output.trim(), "/tmp"));

const failed = spawn("pwd", { cwd: "does-not-exist" });
assert.strictEqual(failed.pid, undefined);
failed.on("error", (error) => assert.strictEqual(error.code, "ENOENT"));
