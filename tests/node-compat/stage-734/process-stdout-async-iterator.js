"use strict";

const assert = require("assert");
const processApi = require("process");

(async () => {
  const iterator = processApi.stdout[Symbol.asyncIterator]();
  assert.strictEqual(typeof iterator.next, "function");
  assert.deepStrictEqual(await iterator.next(), {
    done: true,
    value: undefined,
  });
})();

console.log("process stdout async iterator passed");
