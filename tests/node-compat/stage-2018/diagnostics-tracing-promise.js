const assert = require("assert");
const dc = require("diagnostics_channel");
const channel = dc.tracingChannel("promise-focus");
const events = [];
channel.subscribe({
  start: () => events.push("start"),
  end: () => events.push("end"),
  asyncStart: () => events.push("asyncStart"),
  asyncEnd: () => events.push("asyncEnd")
});
(async () => {
  const result = await channel.tracePromise(
    () => Promise.resolve(42),
    {},
    undefined
  );
  assert.strictEqual(result, 42);
  assert.deepStrictEqual(events, ["start", "end", "asyncStart", "asyncEnd"]);
  console.log("diagnostics promise tracing passed");
})().catch((error) => {
  throw error;
});
