const assert = require("node:assert");

assert.throws(
  () => {
    throw new TypeError("predicate");
  },
  (error) => error instanceof TypeError && error.message === "predicate",
);

console.log("assert predicate validation passed");
