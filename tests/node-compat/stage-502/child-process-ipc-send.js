const assert = require("assert");
const childProcess = require("child_process");

const child = childProcess.fork("persistent.js");
assert.strictEqual(child.send({ value: true }), true);
assert.strictEqual(child.send({ value: true }), true);
assert.strictEqual(child.send({ value: true }), false);
child.kill();
