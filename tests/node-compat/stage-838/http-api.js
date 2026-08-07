"use strict";

const assert = require("assert");
const http = require("node:http");

for (
  const name of [
    "request",
    "get",
    "createServer",
    "validateHeaderName",
    "validateHeaderValue",
    "setMaxIdleHTTPParsers",
  ]
) {
  assert.strictEqual(typeof http[name], "function");
}
for (
  const name of [
    "Agent",
    "ClientRequest",
    "IncomingMessage",
    "Server",
    "ServerResponse",
  ]
) {
  assert.strictEqual(typeof http[name], "function");
}
assert.strictEqual(typeof http.METHODS, "object");
assert.strictEqual(typeof http.STATUS_CODES, "object");

console.log("http api passed");
