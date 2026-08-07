const assert = require("assert");
const domain = require("domain");

const d = new domain.Domain();
const callback = () => {};
d.on("error", (error) => {
  assert.strictEqual(error.message, "foobar");
  assert.strictEqual(error.domain, d);
  assert.strictEqual(error.domainBound, callback);
  assert.strictEqual(error.domainThrown, false);
});
d.intercept(callback)(new Error("foobar"));

const nested = domain.create();
nested.on("error", (error) => assert.strictEqual(error.message, "died"));
nested.run(() => {
  throw new Error("died");
});

console.log("domain intercept nested passed");
