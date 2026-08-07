const assert = require("node:assert");
const http = require("node:http");

const server = http.createServer();
let callbackThis;
server.listen(43210, function () {
  callbackThis = this;
});
assert.strictEqual(callbackThis, undefined);

const duplicate = http.createServer();
let duplicateError;
duplicate.on("error", (error) => {
  duplicateError = error;
});
duplicate.listen(43210);
setImmediate(() => {
  assert.strictEqual(callbackThis, server);
  assert.strictEqual(duplicateError.code, "EADDRINUSE");
  server.close();
  duplicate.close();
  console.log("http listen callback and EADDRINUSE passed");
});
