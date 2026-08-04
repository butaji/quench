"use strict";

const assert = require("assert");
const crypto = require("node:crypto");

const key = crypto.createSecretKey(Buffer.from("secret"));
assert.strictEqual(key.type, "secret");
assert.strictEqual(key.symmetricKeySize, 6);
assert.strictEqual(typeof key.export, "function");
assert.strictEqual(Buffer.from(key.export()).toString(), "secret");

console.log("crypto secret key passed");
