const assert = require("node:assert");
const http = require("node:http");

const expected = new Error("agent socket failed");
const agent = new http.Agent();
agent.createSocket = (_request, _options, callback) => callback(expected);

const request = http
  .request({ agent })
  .on("error", (error) => assert.strictEqual(error, expected))
  .on("close", () => {
    assert.strictEqual(request.destroyed, true);
    console.log("http agent createSocket error passed");
  });

request.end();
