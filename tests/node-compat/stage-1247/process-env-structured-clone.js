const assert = require("node:assert");

process.env.QUENCH_CLONE_VALUE = "present";
const clone = structuredClone(process.env);

assert.deepStrictEqual(clone, { ...process.env });
assert.notStrictEqual(clone, process.env);

delete process.env.QUENCH_CLONE_VALUE;
console.log("process.env structured clone passed");
