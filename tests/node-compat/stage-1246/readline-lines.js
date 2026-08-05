const assert = require("assert");
const { PassThrough } = require("stream");
const readline = require("readline");

const input = new PassThrough();
const lines = [];
readline.createInterface({ input }).on("line", (line) => lines.push(line));
input.end("abc\ndef");
setTimeout(() => assert.deepStrictEqual(lines, ["abc", "def"]), 0);
