"use strict";
const assert = require("assert");
const net = require("net");
assert.throws(() => net.connect(), { code: "ERR_MISSING_ARGS" });
assert.throws(() => net.connect({}), { code: "ERR_MISSING_ARGS" });
