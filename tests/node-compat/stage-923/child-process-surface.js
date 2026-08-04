const assert = require("assert");
const childProcess = require("child_process");

const child = childProcess.spawn(process.execPath, ["-e", ""]);
assert.ok(child instanceof childProcess.ChildProcess);
assert.strictEqual(child.spawnfile, process.execPath);
assert.deepStrictEqual(child.spawnargs, ["-e", ""]);
assert.strictEqual(typeof child.kill, "function");
child.kill();
