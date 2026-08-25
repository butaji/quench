const assert = require("assert");
const dc = require("diagnostics_channel");

const tracing = dc.tracingChannel("rust-tracing");
const seen = [];
const onStart = (message) => { seen.push(["start", message]); };
tracing.subscribe({
  start: onStart,
  end(message) { seen.push(["end", message.result]); },
});
assert.strictEqual(tracing.hasSubscribers, true);
assert.strictEqual(tracing.traceSync(() => 7, { input: true }), 7);
assert.strictEqual(seen[0][0], "start");
assert.strictEqual(seen[1][0], "end");
assert.strictEqual(seen[1][1], 7);
assert.strictEqual(tracing.unsubscribe({ start: onStart }), true);
