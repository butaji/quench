const assert = require("assert");
const net = require("net");

const original = net.getDefaultAutoSelectFamily();
net.setDefaultAutoSelectFamily(true);
assert.strictEqual(net.getDefaultAutoSelectFamily(), true);
net.setDefaultAutoSelectFamily(false);
assert.strictEqual(net.getDefaultAutoSelectFamily(), false);
assert.throws(() => net.setDefaultAutoSelectFamily("true"), {
  code: "ERR_INVALID_ARG_TYPE",
});

const originalTimeout = net.getDefaultAutoSelectFamilyAttemptTimeout();
net.setDefaultAutoSelectFamilyAttemptTimeout(125);
assert.strictEqual(net.getDefaultAutoSelectFamilyAttemptTimeout(), 125);
assert.throws(() => net.setDefaultAutoSelectFamilyAttemptTimeout(0), {
  code: "ERR_OUT_OF_RANGE",
});
net.setDefaultAutoSelectFamilyAttemptTimeout(originalTimeout);
net.setDefaultAutoSelectFamily(original);
console.log("net auto select family state passed");
