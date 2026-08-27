const assert = require("assert");
const { ChildProcess } = require("child_process");

assert.strictEqual(typeof ChildProcess, "function");
const child = new ChildProcess();
assert.throws(() => child.spawn(undefined), { code: "ERR_INVALID_ARG_TYPE" });
assert.throws(() => child.spawn({ file: 1 }), { code: "ERR_INVALID_ARG_TYPE" });
child.spawn({ file: process.execPath, args: [], stdio: "pipe" });
assert(Number.isInteger(child.pid));
assert.strictEqual(child.kill("SIGTERM"), true);
