"use strict";
const assert = require("assert");
const { AsyncResource } = require("async_hooks");

assert.throws(() => new AsyncResource(), { code: "ERR_INVALID_ARG_TYPE", name: "TypeError" });
assert.throws(() => new AsyncResource(""), { code: "ERR_ASYNC_TYPE", name: "TypeError" });
assert.throws(() => new AsyncResource("type", -4), { code: "ERR_INVALID_ASYNC_ID", name: "RangeError" });
assert.throws(() => new AsyncResource("type", Math.PI), { code: "ERR_INVALID_ASYNC_ID", name: "RangeError" });
