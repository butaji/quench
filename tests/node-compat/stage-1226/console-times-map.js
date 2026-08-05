const assert = require("assert");

console.time("stable");
const start = console._times.get("stable");
console.time("stable");
assert.strictEqual(console._times.get("stable"), start);
console.timeEnd("stable");
