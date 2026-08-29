"use strict";
const assert = require("assert");
const { setImmediate, setTimeout } = require("timers/promises");

for (const value of [1, "", false, Infinity]) {
  assert.rejects(setImmediate(0, value), { code: "ERR_INVALID_ARG_TYPE" });
  assert.rejects(setTimeout(0, undefined, value), { code: "ERR_INVALID_ARG_TYPE" });
}
for (const signal of [1, "", false, Infinity, null, {}]) {
  assert.rejects(setImmediate(0, { signal }), { code: "ERR_INVALID_ARG_TYPE" });
  assert.rejects(setTimeout(0, undefined, { signal }), { code: "ERR_INVALID_ARG_TYPE" });
}
for (const ref of [1, "", Infinity, null, {}]) {
  assert.rejects(setImmediate(0, { ref }), { code: "ERR_INVALID_ARG_TYPE" });
  assert.rejects(setTimeout(0, undefined, { ref }), { code: "ERR_INVALID_ARG_TYPE" });
}
