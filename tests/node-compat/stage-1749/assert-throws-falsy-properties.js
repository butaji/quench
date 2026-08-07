const assert = require("assert");

assert.throws(
  () => assert.ifError({ constructor: null, message: "" }),
  { message: "ifError got unwanted exception: " },
);

assert.throws(() => assert.ifError(false), {
  message: "ifError got unwanted exception: false",
});

console.log("assert throws falsy properties ok");
