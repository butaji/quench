"use strict";

const assert = require("assert");
const processApi = require("process");

for (
  const name of [
    "_rawDebug",
    "_debugProcess",
    "_debugEnd",
    "_startProfilerIdleNotifier",
    "_stopProfilerIdleNotifier",
    "_tickCallback",
  ]
) {
  assert.strictEqual(typeof processApi[name], "function");
}

console.log("process debug methods passed");
