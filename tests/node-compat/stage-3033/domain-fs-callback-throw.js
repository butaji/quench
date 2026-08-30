"use strict";

const assert = require("assert");
const domain = require("domain").create();
const fs = require("fs");
let seen = 0;

domain.on("error", (error) => {
  assert.strictEqual(error.message, "boom");
  assert.strictEqual(error.domain, domain);
  seen++;
});
domain.run(() => {
  fs.stat("file that does not exist", (error) => {
    throw new Error("boom");
  });
});
setImmediate(() => assert.strictEqual(seen, 1));
