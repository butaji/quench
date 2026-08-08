const assert = require("assert");
const chalk = require("chalk");

const value = chalk.red.bold("quench-node");
assert.strictEqual(typeof value, "string");
assert(value.includes("quench-node"));
