"use strict";

const assert = require("assert");
const dc = require("diagnostics_channel");

const tracing = dc.tracingChannel("stage-3005");
const callback = () => {};

assert.strictEqual(tracing.hasSubscribers, false);
tracing.asyncEnd.subscribe(callback);
assert.strictEqual(tracing.hasSubscribers, true);
tracing.asyncEnd.unsubscribe(callback);
assert.strictEqual(tracing.hasSubscribers, false);

const bounded = dc.boundedChannel("stage-3005-bounded");
bounded.start.subscribe(callback);
assert.strictEqual(bounded.hasSubscribers, true);
