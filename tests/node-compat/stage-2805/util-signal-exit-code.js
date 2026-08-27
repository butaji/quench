"use strict";
const assert = require("assert");
const { convertProcessSignalToExitCode } = require("util");

assert.strictEqual(convertProcessSignalToExitCode("SIGTERM"), 143);
assert.strictEqual(convertProcessSignalToExitCode("SIGINT"), 130);
assert.throws(() => convertProcessSignalToExitCode("INVALID"), {
  code: "ERR_INVALID_ARG_VALUE",
});
