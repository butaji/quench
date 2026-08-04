"use strict";

const assert = require("assert");
const diagnosticsApi = require("node:diagnostics_channel");

for (const name of ["channel", "subscribe", "unsubscribe", "hasSubscribers"]) {
  assert.strictEqual(typeof diagnosticsApi[name], "function");
}
const channel = diagnosticsApi.channel("compatibility");
assert.strictEqual(typeof channel.publish, "function");
assert.strictEqual(typeof channel.subscribe, "function");
assert.strictEqual(diagnosticsApi.hasSubscribers("compatibility"), false);

console.log("diagnostics channel api passed");
