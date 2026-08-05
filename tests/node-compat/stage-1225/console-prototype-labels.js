const assert = require("assert");

assert.doesNotThrow(() => {
  console.time("constructor");
  console.timeEnd("constructor");
  console.time("__proto__");
  console.timeEnd("__proto__");
});
