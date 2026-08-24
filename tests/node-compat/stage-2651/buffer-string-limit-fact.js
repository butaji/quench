"use strict";
const assert = require("assert");
const { constants } = require("buffer");
assert.strictEqual(constants.MAX_STRING_LENGTH, 536870888);
assert(constants.MAX_STRING_LENGTH < constants.MAX_LENGTH);
