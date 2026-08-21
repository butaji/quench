const assert = require("assert");
const consoleApi = require("node:console");

assert.strictEqual(typeof consoleApi.dir, "function");
assert.strictEqual(typeof consoleApi.createTask, "function");
const task = consoleApi.createTask("example");
assert.strictEqual(task.name, "example");
assert.strictEqual(task.run((value) => value + 1, 4), 5);

console.log("console module backlog passed");
