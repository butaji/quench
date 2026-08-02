const assert = require("assert");
const util = require("util");

const date = new Date("2023-10-01T00:00:00Z");
assert.strictEqual(util.format("%s", date), "2023-10-01T00:00:00.000Z");
