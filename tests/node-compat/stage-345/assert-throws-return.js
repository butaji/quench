const assert = require("assert");
const error = assert.throws(() => {
  throw new TypeError("expected");
}, TypeError);
if (error.message !== "expected") throw new Error(error.message);
