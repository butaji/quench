const assert = require("assert");

assert.throws(
  () => {
    throw new Error("boom");
  },
  (error) => error.message === "boom",
);
