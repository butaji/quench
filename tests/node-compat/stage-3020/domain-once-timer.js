"use strict";
const assert = require("assert");
const d = require("domain").create();
let errors = 0;
d.once("error", (error) => {
  assert.strictEqual(error.message, "boom");
  errors++;
});
d.once("error", () => {
  errors++;
});
d.run(() => setImmediate(() => { throw new Error("boom"); }));
setImmediate(() => assert.strictEqual(errors, 2));
