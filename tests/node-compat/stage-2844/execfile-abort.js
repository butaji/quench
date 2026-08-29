const assert = require("assert");
const { execFile } = require("child_process");

const controller = new AbortController();
const signal = controller.signal;
let calls = 0;
execFile(process.execPath, ["echo", "ok"], { signal }, (error) => {
  calls++;
  assert.strictEqual(error.code, "ABORT_ERR");
});
controller.abort();
setImmediate(() => assert.strictEqual(calls, 1));
