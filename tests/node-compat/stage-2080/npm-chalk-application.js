const assert = require("assert");
const chalk = require("chalk");

const value = chalk.red.bold("quench");
assert.strictEqual(typeof value, "string");
assert(value.includes("quench"));
