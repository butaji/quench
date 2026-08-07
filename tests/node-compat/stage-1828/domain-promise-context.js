const assert = require("assert");
const domain = require("domain");

const d = domain.create();
d.run(() => {
  Promise.resolve().then(() => {
    assert.strictEqual(process.domain, d);
    console.log("domain Promise context passed");
  });
});
