const assert = require("assert");
const childProcess = require("child_process");

childProcess.exec("ok", { encoding: "buffer" }, (error, stdout, stderr) => {
  assert.strictEqual(error, null);
  assert.strictEqual(Buffer.isBuffer(stdout), true);
  assert.strictEqual(Buffer.isBuffer(stderr), true);
  console.log("child process exec encoding passed");
});
