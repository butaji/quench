const assert = require("assert");
const http = require("http");

const request = http.request({ host: "127.0.0.1", port: 1 });
assert.ok(request instanceof http.ClientRequest);
request.on("error", () => {});
request.destroy();
