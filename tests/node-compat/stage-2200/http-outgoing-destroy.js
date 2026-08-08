const assert = require("assert");
const { OutgoingMessage } = require("http");

const message = new OutgoingMessage();
const error = new Error("destroyed");

assert.strictEqual(message.destroyed, false);
assert.strictEqual(message.closed, false);
message.on("close", () => {
  assert.strictEqual(message.destroyed, true);
  assert.strictEqual(message.closed, true);
  assert.strictEqual(message.errored, error);
});
message.destroy(error);
assert.strictEqual(message.destroyed, true);
assert.strictEqual(message.errored, error);
