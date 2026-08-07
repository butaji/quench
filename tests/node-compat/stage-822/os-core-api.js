"use strict";

const assert = require("assert");
const osApi = require("node:os");

assert.strictEqual(typeof osApi.EOL, "string");
assert.strictEqual(typeof osApi.devNull, "string");
for (
  const name of [
    "arch",
    "platform",
    "cpus",
    "freemem",
    "totalmem",
    "homedir",
    "tmpdir",
    "type",
    "release",
    "endianness",
    "loadavg",
    "networkInterfaces",
    "userInfo",
  ]
) {
  assert.strictEqual(typeof osApi[name], "function");
}
assert.strictEqual(typeof osApi.arch(), "string");
assert.strictEqual(Array.isArray(osApi.cpus()), true);

console.log("os core api passed");
