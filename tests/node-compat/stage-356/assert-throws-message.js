const assert = require("assert");
assert.throws(() => assert.throws(() => {}, TypeError, "fhqwhgads"), {
  message: "Missing expected exception (TypeError): fhqwhgads",
});
