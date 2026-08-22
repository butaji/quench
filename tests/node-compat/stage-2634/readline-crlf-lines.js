"use strict";

const assert = require("assert");
const readline = require("readline");

const lines = [];
const rl = readline.createInterface({ input: "first\r\nsecond\r\n" });
rl.on("line", (line) => lines.push(line));
rl.on("close", () => {
  assert.deepStrictEqual(lines, ["first", "second", ""]);
  console.log("readline CRLF lines passed");
});
