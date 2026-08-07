"use strict";

const assert = require("assert");
const dns = require("node:dns");
const dnsPromises = require("node:dns/promises");

for (
  const name of [
    "lookup",
    "resolve",
    "resolve4",
    "resolve6",
    "reverse",
    "getDefaultResultOrder",
    "setDefaultResultOrder",
  ]
) {
  assert.strictEqual(typeof dns[name], "function");
}
for (const name of ["lookup", "resolve", "resolve4", "resolve6", "reverse"]) {
  assert.strictEqual(typeof dnsPromises[name], "function");
}
assert.strictEqual(typeof dns.promises, "object");

console.log("dns api passed");
