const assert = require("assert");

const names = {
  IndexSizeError: 1,
  HierarchyRequestError: 3,
  InvalidCharacterError: 5,
  NotFoundError: 8,
  NotSupportedError: 9,
  InvalidStateError: 11,
  SyntaxError: 12,
  SecurityError: 18,
  NetworkError: 19,
  AbortError: 20,
  QuotaExceededError: 22,
  TimeoutError: 23,
  DataCloneError: 25,
};
for (const [name, code] of Object.entries(names)) {
  const error = new DOMException("message", name);
  assert.strictEqual(error.code, code);
  assert.strictEqual(String(error), `${name}: message`);
}
assert.strictEqual(new DOMException("message", "Unknown").code, 0);
assert.strictEqual(DOMException.ABORT_ERR, 20);

console.log("DOMException code table passed");
