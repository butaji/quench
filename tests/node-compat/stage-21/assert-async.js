const assert = require("assert");
assert.rejects(Promise.reject(new TypeError("expected")), {
  name: "TypeError",
});
assert.doesNotReject(Promise.resolve("ok"));
