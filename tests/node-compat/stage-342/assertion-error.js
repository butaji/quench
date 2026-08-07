const assert = require("assert");
if (!(assert.AssertionError.prototype instanceof Error)) {
  throw new Error("AssertionError prototype");
}
try {
  assert(false);
} catch (error) {
  if (!(error instanceof assert.AssertionError)) throw error;
}
