"use strict";

const assert = require("assert");
const { AsyncResource } = require("async_hooks");

const resource = new AsyncResource("TestResource");
assert.throws(() => resource.domain, {
  code: "ERR_ASYNC_RESOURCE_DOMAIN_REMOVED",
  message:
    "The domain property on AsyncResource has been removed. " +
    "Use AsyncLocalStorage instead.",
});
