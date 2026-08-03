"use strict";

const assert = require("assert");
const diagnostics = require("diagnostics_channel");

const channel = diagnostics.channel("quench.test");
let received;
const subscriber = (message, context) => {
  received = { message, context };
};
assert.strictEqual(channel.hasSubscribers, false);
channel.subscribe(subscriber);
assert.strictEqual(diagnostics.hasSubscribers("quench.test"), true);
channel.publish({ ok: true }, "context");
assert.deepStrictEqual(received, { message: { ok: true }, context: "context" });
channel.unsubscribe(subscriber);
assert.strictEqual(channel.hasSubscribers, false);
assert.ok(diagnostics.tracingChannel("quench.trace").start);

console.log("diagnostics channel passed");
