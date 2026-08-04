"use strict";

const assert = require("assert");
const processApi = require("process");

let received;
processApi.on("stage-672", (...args) => {
  received = args;
});
assert.strictEqual(processApi.emit("stage-672", "first", 2), true);
assert.deepStrictEqual(received, ["first", 2]);
assert.strictEqual(processApi.emit("stage-672-unhandled"), false);
processApi.removeAllListeners("stage-672");

console.log("process emit passed");
