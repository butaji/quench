"use strict";

const assert = require("assert");
const { Domain } = require("domain");

const domain = new Domain();
let seen = 0;
domain.on("error", (error) => {
  assert.strictEqual(error.domain, domain);
  seen++;
});

setTimeout(
  domain.bind(() => {
    throw new Error("bound");
  }),
  0,
);

const intercepted = domain.intercept((value) => {
  assert.strictEqual(value, "ok");
});
intercepted(null, "ok");
intercepted(new Error("intercepted"));
assert.strictEqual(seen, 1);

setTimeout(() => assert.strictEqual(seen, 2), 1);
