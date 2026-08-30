"use strict";

const assert = require("assert");
const domain = require("domain").create();

domain.run(() => {
  Promise.resolve().then(() => {
    assert.strictEqual(process.domain, domain);
  });
});
