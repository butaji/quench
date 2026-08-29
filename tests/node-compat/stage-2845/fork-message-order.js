const assert = require("assert");
const { fork } = require("child_process");
const fixtures = require("../../node/test/common/fixtures");

const child = fork(fixtures.path("child-process-message-and-exit.js"));
const events = [];
child.on("message", (message) => events.push(`message:${message}`));
child.on("exit", () => events.push("exit"));
child.on("close", () => {
  events.push("close");
  assert.deepStrictEqual(events, ["message:hello", "exit", "close"]);
});
