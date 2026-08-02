const assert = require("assert");
const { executionAsyncResource } = require("async_hooks");

const resource = executionAsyncResource();
resource.value = "captured";
setTimeout(() => {
  assert.strictEqual(executionAsyncResource().value, "captured");
}, 0);
