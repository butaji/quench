const assert = require("assert");
const http = require("http");

const agent = http.globalAgent;
assert.strictEqual(agent.defaultPort, 80);
assert.strictEqual(agent.protocol, "http:");
assert.strictEqual(agent.keepAliveMsecs, 1000);
assert.strictEqual(agent.agentKeepAliveTimeoutBuffer, 1000);
assert.strictEqual(
  agent.getName({ host: "x", port: 80, localAddress: "127.0.0.1", family: 4 }),
  "x:80:127.0.0.1:4",
);

const custom = new http.Agent({ keepAliveMsecs: 250 });
assert.strictEqual(custom.keepAliveMsecs, 250);

console.log("http Agent metadata passed");
