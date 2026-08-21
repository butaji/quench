const assert = require("assert");
const http = require("http");

const request = http.request({ host: "127.0.0.1", port: 1 });
let closes = 0;
request.on("close", () => {
  closes += 1;
  assert.strictEqual(closes, 1);
});
request.on("error", () => {});
request.destroy();
request.destroy();
