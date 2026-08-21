const assert = require("assert");

(async () => {
  const events = [];
  setTimeout(() => events.push("timeout"), 20);
  setImmediate(() => events.push("immediate"));
  queueMicrotask(() => events.push("microtask"));
  await new Promise((resolve) => setTimeout(resolve, 30));
  assert.deepStrictEqual(events.slice(0, 2), ["microtask", "immediate"]);
  assert.strictEqual(events.includes("timeout"), true);
  console.log("timer job ordering passed");
})();
