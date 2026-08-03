const assert = require("node:assert");
assert.ok(true);
assert.equal("4", 4);
assert.notEqual("4", 5);
assert.notStrictEqual("4", 4);
assert.doesNotThrow(() => {});
assert.match("quench-node", /node$/);
