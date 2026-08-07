const assert = require("assert");
const http = require("http");

const request = http.request({ host: "example.test", path: "/" });
assert.strictEqual(request.agent, http.globalAgent);
request.destroy();

const custom = new http.Agent({ keepAlive: false });
const customRequest = http.request({
  host: "example.test",
  path: "/",
  agent: custom,
});
assert.strictEqual(customRequest.agent, custom);
customRequest.destroy();

console.log("http request Agent defaults passed");
