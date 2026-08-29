"use strict";

const assert = require("assert");
const { setInterval } = require("timers/promises");

for (const options of [
  1,
  "",
  false,
  Infinity,
  null,
  true,
  { ref: 1 },
  { signal: {} },
]) {
  const iterator = setInterval(1, undefined, options);
  assert.rejects(iterator.next(), { code: "ERR_INVALID_ARG_TYPE" });
}
