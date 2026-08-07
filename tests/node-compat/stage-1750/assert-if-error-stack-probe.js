const assert = require("assert");

let caught;
try {
  assert.throws(() => assert.ifError(null));
} catch (error) {
  caught = error;
}

assert(caught);
console.log(JSON.stringify({
  message: caught.message,
  containsThrows: caught.stack.includes("throws"),
  stack: caught.stack,
}));
