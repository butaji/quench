"use strict";

const assert = require("assert");
const net = require("node:net");

const list = new net.BlockList();
for (const name of ["addAddress", "addRange", "addSubnet", "check"]) {
  assert.strictEqual(typeof list[name], "function");
}
assert.strictEqual(typeof list.rules, "object");
list.addAddress("192.0.2.1");
assert.strictEqual(list.check("192.0.2.1"), true);

console.log("net blocklist api passed");
