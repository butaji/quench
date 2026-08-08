const assert = require("assert");
const http = require("http");

const agent = new http.Agent({ maxSockets: 1 });
const first = http.get({ host: "localhost", port: 1, agent });
const second = http.get({ host: "localhost", port: 1, agent });
const name = agent.getName({ host: "localhost", port: 1 });

assert.strictEqual(agent.requests[name].length, 1);
first.destroy();
second.destroy();
agent.destroy();
console.log("http agent request queue passed");
