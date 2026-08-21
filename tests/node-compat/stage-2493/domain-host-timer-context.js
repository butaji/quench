const assert = require("assert");
const domain = require("domain");

const active = domain.create();
let errors = 0;
active.on("error", (error) => {
  errors++;
  assert.strictEqual(error.message, "scheduled failure");
  assert.strictEqual(error.domain, active);
});

active.run(() => {
  setTimeout(() => {
    assert.strictEqual(process.domain, active);
    throw new Error("scheduled failure");
  }, 0);
});

setImmediate(() => assert.strictEqual(errors, 1));
