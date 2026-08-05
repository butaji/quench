const assert = require("node:assert");

const params = new URLSearchParams();
params.append("a", "\ud83d");
params.append("a", "\ude00");
params.append("a", "\ud83d\ude00");
assert.strictEqual(params.toString(), "a=%EF%BF%BD&a=%EF%BF%BD&a=%F0%9F%98%80");
console.log("URLSearchParams surrogate encoding passed");
