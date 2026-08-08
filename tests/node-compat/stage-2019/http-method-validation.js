const assert = require("assert");
const http = require("http");
assert.strictEqual(typeof http.OutgoingMessage, "function");
const outgoing = new http.OutgoingMessage();
assert.strictEqual(outgoing.writableObjectMode, false);
assert(outgoing.writableHighWaterMark > 0);

for (const method of [-1, {}, true, false, [], Symbol("method")]) {
  assert.throws(() => http.request({ method }), {
    code: "ERR_INVALID_ARG_TYPE",
    name: "TypeError"
  });
}

assert.doesNotThrow(() => http.request({ method: undefined }));
console.log("HTTP method validation passed");
