"use strict";

const assert = require("assert");
const processApi = require("process");

const values = [];
const first = (value) => values.push(`first:${value}`);
const second = (value) => values.push(`second:${value}`);
processApi.on("stage-644", first);
processApi.on("stage-644", second);

assert.strictEqual(processApi.emit("stage-644", "payload"), true);
assert.deepStrictEqual(values, ["first:payload", "second:payload"]);
processApi.removeAllListeners("stage-644");
assert.strictEqual(processApi.emit("stage-644", "payload"), false);

console.log("process emit fanout passed");
