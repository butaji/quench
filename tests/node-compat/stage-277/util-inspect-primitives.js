const { inspect } = require("util");

if (inspect("hello") !== "'hello'") throw new Error("string inspect failed");
if (inspect(function named() {}) !== "[Function: named]") {
  throw new Error("named function inspect failed");
}
if (inspect(() => {}) !== "[Function (anonymous)]") {
  throw new Error("anonymous function inspect failed");
}
