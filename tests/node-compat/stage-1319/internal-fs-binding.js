const assert = require("node:assert");
const { internalBinding } = require("internal/test/binding");

const context = {};
internalBinding("fs").openFileHandle(__filename, 0, 0o666, undefined, context);
assert.strictEqual(context.errno, undefined);
console.log("internal fs binding passed");
