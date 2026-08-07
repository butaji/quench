const assert = require("assert");
const domain = require("domain");

const d = new domain.Domain();
assert.strictEqual(typeof d.on, "function");
assert.strictEqual(typeof d.emit, "function");
assert.strictEqual(domain.createDomain instanceof Function, true);
d.on("error", (error) => {
  assert.strictEqual(error.message, "foobar");
  assert.strictEqual(error.domain, d);
  assert.strictEqual(error.domainThrown, true);
  console.log("domain bind error passed");
});
setTimeout(
  d.bind(() => {
    throw new Error("foobar");
  }),
  1,
);
