const assert = require("node:assert");
const http = require("node:http");

const request = new http.IncomingMessage(null);
assert.strictEqual(request.signal, request.signal);
assert.strictEqual(request.signal.aborted, false);
request.destroy();
assert.strictEqual(request.signal.aborted, true);
assert.strictEqual(request.aborted, true);
console.log("http request signal surface passed");
