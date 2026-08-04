"use strict";

const assert = require("assert");
const domain = require("node:domain");

for (const name of ["create", "createDomain", "active"]) {
  assert.ok(name in domain);
}
assert.strictEqual(typeof domain.create, "function");
assert.strictEqual(typeof domain.createDomain, "function");
assert.strictEqual(typeof domain.active, "object");

const instance = domain.create();
assert.strictEqual(typeof instance.add, "function");
assert.strictEqual(typeof instance.remove, "function");
assert.strictEqual(typeof instance.run, "function");

console.log("domain api passed");
