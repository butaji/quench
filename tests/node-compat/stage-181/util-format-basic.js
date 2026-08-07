const util = require("util");

if (util.format([0]) !== "[ 0 ]") throw new Error("array format mismatch");
if (util.format({ foo: 42 }) !== "{ foo: 42 }") {
  throw new Error("object format mismatch");
}
if (util.format("%i %f", "42.5", "1.5") !== "42 1.5") {
  throw new Error("numeric format mismatch");
}
if (util.format("%d", 42.0) !== "42") {
  throw new Error(`d format mismatch: ${util.format("%d", 42.0)}`);
}
if (util.format("%d") !== "%d") {
  throw new Error("missing numeric argument mismatch");
}
if (util.format("%s", undefined) !== "undefined") {
  throw new Error("explicit undefined mismatch");
}
if (util.format("%d", "") !== "0") {
  throw new Error(`empty number mismatch: ${util.format("%d", "")}`);
}
if (util.format("%d", -0) !== "-0") throw new Error("negative zero mismatch");
const symbol = Symbol("foo");
if (util.format(symbol) !== "Symbol(foo)") {
  throw new Error("symbol format mismatch");
}
if (util.format("%s", symbol) !== "Symbol(foo)") {
  throw new Error("symbol string mismatch");
}
if (util.format("%j", symbol) !== "undefined") {
  throw new Error("symbol json mismatch");
}
if (util.format("foo", "bar", "baz") !== "foo bar baz") {
  throw new Error("extra argument format mismatch");
}
