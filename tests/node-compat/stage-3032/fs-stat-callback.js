"use strict";

const assert = require("assert");
const fs = require("fs");

fs.stat(__filename, (error, stats) => {
  assert.ifError(error);
  assert.strictEqual(typeof stats.isFile, "function");
});
