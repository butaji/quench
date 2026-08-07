const assert = require("assert");
const domain = require("domain");

const d = domain.create();
d.on("error", (error) => {
  assert.strictEqual(error.message, "timer-error");
  assert.strictEqual(error.domain, d);
  console.log("domain timer context passed");
});
d.run(() => {
  setTimeout(() => {
    throw new Error("timer-error");
  }, 1);
});
