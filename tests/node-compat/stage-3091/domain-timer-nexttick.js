"use strict";
const assert = require("assert");

setTimeout(() => {
  const domain = require("domain").create();
  domain.run(() => {
    process.nextTick(() => {
      assert.strictEqual(process.domain, domain);
    });
  });
}, 1);
