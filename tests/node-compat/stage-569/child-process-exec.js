const assert = require("assert");
const childProcess = require("child_process");

for (const name of ["exec", "execFile", "execSync", "execFileSync"]) {
  assert.strictEqual(typeof childProcess[name], "function");
}

childProcess.exec("echo ok", (error, stdout, stderr) => {
  assert.strictEqual(error, null);
  assert.strictEqual(stdout, "");
  assert.strictEqual(stderr, "");
  console.log("child process exec passed");
});
