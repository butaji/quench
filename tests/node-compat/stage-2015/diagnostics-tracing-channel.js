const assert = require("assert");
const diagnostics = require("diagnostics_channel");

const channel = diagnostics.tracingChannel("focused-tracing");
const events = [];
channel.subscribe({
  start: (message) => events.push(["start", message.value]),
  end: (message) => events.push(["end", message.result]),
  error: (message) => events.push(["error", message.error.message])
});

assert.strictEqual(typeof channel.traceSync, "function");
assert.strictEqual(channel.hasSubscribers, true);
assert.strictEqual(
  channel.traceSync(() => 42, { value: "sync" }),
  42
);
assert.deepStrictEqual(events, [
  ["start", "sync"],
  ["end", 42]
]);

assert.throws(
  () =>
    channel.traceSync(() => {
      throw new Error("boom");
    }, {}),
  /boom/
);
assert(events.some((event) => event[0] === "error" && event[1] === "boom"));

const callbackResult = channel.traceCallback(
  (value, callback) => callback(null, value + 1),
  -1,
  {},
  undefined,
  41,
  (error, value) => assert.strictEqual(value, 42)
);
assert.strictEqual(callbackResult, undefined);

console.log("diagnostics tracing channel passed");
