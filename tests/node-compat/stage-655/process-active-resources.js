"use strict";

const assert = require("assert");
const processApi = require("process");

const resources = processApi.getActiveResourcesInfo();
assert(Array.isArray(resources));
for (const resource of resources) assert.strictEqual(typeof resource, "string");

console.log("process active resources passed");
