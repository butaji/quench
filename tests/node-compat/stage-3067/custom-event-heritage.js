"use strict";

const assert = require("assert");
const { CustomEvent } = require("internal/event_target");

const SubEvent = class extends CustomEvent {};
assert.strictEqual(new SubEvent("x").type, "x");
