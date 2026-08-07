"use strict";
const assert = require("assert");

const deferred = Promise.withResolvers();
assert.ok(deferred.promise instanceof Promise);
deferred.resolve(42);
deferred.promise.then((value) => {
  assert.strictEqual(value, 42);
  console.log("promise withResolvers passed");
});
