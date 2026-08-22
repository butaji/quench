const assert = require("node:assert");
const dnsPromises = require("node:dns/promises");

assert.strictEqual(typeof dnsPromises.resolve, "function");
assert.strictEqual(typeof dnsPromises.resolve4, "function");
assert.strictEqual(typeof dnsPromises.lookup, "function");

const result = dnsPromises.resolve4("localhost");
assert.strictEqual(typeof result.then, "function");
console.log("DNS promises module passed");
