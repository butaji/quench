const assert = require("assert");
try {
  assert.fail("expected failure");
  throw new Error("assert.fail returned");
} catch (error) {
  if (error.code !== "ERR_ASSERTION") throw error;
  if (error.message !== "expected failure") throw error;
}
