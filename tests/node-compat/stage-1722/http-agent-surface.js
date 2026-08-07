const assert = require("node:assert");
const http = require("node:http");

const agent = new http.Agent({
  keepAlive: true,
  maxSockets: 3,
  maxFreeSockets: 2,
});
assert.strictEqual(agent.keepAlive, true);
assert.strictEqual(agent.maxSockets, 3);
assert.strictEqual(agent.maxFreeSockets, 2);
assert.strictEqual(
  agent.getName({ host: "example.com", port: 80 }),
  "example.com:80:",
);
assert.strictEqual(
  agent.getName({ socketPath: "/tmp/http.sock", host: "ignored" }),
  "ignored:::/tmp/http.sock",
);
assert.deepStrictEqual(agent.getCurrentStatus(), {
  createSocketCount: 0,
  closeSocketCount: 0,
  timeoutSocketCount: 0,
  requestCount: 0,
  freeSockets: {},
  sockets: {},
  requests: {},
});
assert.strictEqual(agent.destroy(), agent);
console.log("http Agent surface passed");
