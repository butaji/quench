"use strict";

const assert = require("assert");
const repl = require("repl");

const output = {
  text: "",
  write(value) {
    output.text += value;
  },
};
const server = repl.start({ prompt: "quench> ", output });
assert.strictEqual(output.text, "quench> ");
server.eval("1 + 2", {}, (error, value) => {
  assert.ifError(error);
  assert.strictEqual(value, 3);
});
server.close();
assert.strictEqual(server.closed, true);

console.log("repl passed");
