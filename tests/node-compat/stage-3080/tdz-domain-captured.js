"use strict";
const assert = require("assert");
const domain = require("domain");
const d = domain.create();
d.on("error", (e) => { throw e; });
d.run(() => {
  setTimeout(() => {
    assert.strictEqual(timeout, "ok");
  }, 0);
});
const timeout = "ok";
