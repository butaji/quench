"use strict";
const assert = require("assert");
const { internalBinding } = require("internal/test/binding");

const now = internalBinding("timers").getLibuvNow();
assert.strictEqual(typeof now, "number");
assert(now >= 0 && now < 0x3ffffff);
