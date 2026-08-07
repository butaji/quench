const assert = require("assert");
const dc = require("diagnostics_channel");
assert.strictEqual(typeof dc.Channel, "function");
assert.ok(dc.channel("instance") instanceof dc.Channel);
const nodeDc = require("node:diagnostics_channel");
assert.strictEqual(typeof nodeDc.Channel, "function");
assert.ok(nodeDc.channel("node-instance") instanceof nodeDc.Channel);
let publishedName;
const named = nodeDc.channel("published-name");
named.subscribe((message, value) => {
  publishedName = value;
});
named.publish({});
assert.strictEqual(publishedName, "published-name");

assert.strictEqual(typeof dc.BoundedChannel, "function");
const bounded = dc.boundedChannel("focused");
assert.strictEqual(bounded.start.name, "tracing:focused:start");
assert.strictEqual(bounded.end.name, "tracing:focused:end");
const events = [];
const handlers = {
  start: (value) => events.push(["start", value]),
  end: (value) => events.push(["end", value]),
};
bounded.subscribe(handlers);
const context = { value: 1 };
bounded.run(context, () => events.push(["body", context]));
assert.deepStrictEqual(events.map(([name]) => name), ["start", "body", "end"]);
assert.strictEqual(bounded.unsubscribe(handlers), true);
