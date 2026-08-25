"use strict";
const assert = require("assert");
const timers = require("timers");
const promise = require("util").promisify(timers.setTimeout)(0, "ok");
promise.then((value) => assert.strictEqual(value, "ok"));
