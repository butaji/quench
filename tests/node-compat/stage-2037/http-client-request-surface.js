const assert = require("assert");
const { ClientRequest } = require("http");

for (const options of [
  { createConnection: () => {} },
  { method: "", createConnection: () => {} },
  { path: "", createConnection: () => {} }
]) {
  const request = new ClientRequest(options);
  assert.strictEqual(request.path, "/");
  assert.strictEqual(request.method, "GET");
  assert.strictEqual(request.end(), request);
}

console.log("http ClientRequest surface passed");
