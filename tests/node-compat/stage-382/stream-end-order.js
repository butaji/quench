const assert = require("assert");
const { Writable } = require("stream");
const order = [];
const writable = new Writable();
writable.on("finish", () => order.push("finish"));
writable.end("done", () => {
  order.push("callback");
  assert.deepStrictEqual(order, ["finish", "callback"]);
});
