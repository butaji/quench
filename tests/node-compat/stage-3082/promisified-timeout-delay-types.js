"use strict";

const assert = require("assert");
const { promisify } = require("util");
const timers = require("timers");

const setPromiseTimeout = promisify(timers.setTimeout);

for (const delay of ["", false]) {
  assert.rejects(setPromiseTimeout(delay, null, {}), {
    code: "ERR_INVALID_ARG_TYPE",
  });
}
