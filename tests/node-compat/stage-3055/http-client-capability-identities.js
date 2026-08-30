const assert = require("assert");
const http = require("http");

assert.strictEqual(typeof http.request, "function");
assert.strictEqual(typeof http.get, "function");
assert.strictEqual(typeof http.Agent, "function");
