const assert = require("assert");
const consoleApi = require("node:console");

assert.strictEqual(typeof consoleApi.dir, "function");
assert.strictEqual(typeof consoleApi.createTask, "function");
const task = consoleApi.createTask("example");
assert.strictEqual(task.name, "example");
assert.strictEqual(task.run((left, right) => left + right, 4, 5), 9);
assert.strictEqual(task.run(() => undefined), undefined);

console.log("console module backlog passed");
