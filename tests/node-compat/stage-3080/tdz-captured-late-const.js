"use strict";
const assert = require("assert");
let seen = false;
setTimeout(() => {
  seen = timeout;
}, 0);
const timeout = "ok";
setTimeout(() => assert.strictEqual(seen, "ok"), 5);
