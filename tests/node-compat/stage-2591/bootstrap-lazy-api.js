"use strict";

const assert = require("node:assert");
const events = require("node:events");
const url = require("node:url");

assert.strictEqual(typeof events.EventEmitter, "function");
assert.strictEqual(typeof url.URL, "function");
assert.strictEqual(new url.URL("https://example.test/").hostname, "example.test");

console.log("bootstrap lazy API surface: ok");
