"use strict";

const assert = require("assert");
const sqlite = require("node:sqlite");

assert.strictEqual(typeof sqlite.DatabaseSync, "function");
assert.strictEqual(typeof sqlite.StatementSync, "function");
assert.strictEqual(typeof sqlite.constants, "object");

console.log("sqlite api passed");
