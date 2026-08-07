const assert = require("assert");
const fs = require("fs");
const promises = require("fs/promises");

assert.strictEqual(promises, fs.promises);
assert.strictEqual(promises.constants, fs.constants);
