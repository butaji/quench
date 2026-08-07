const assert = require("node:assert");
const url = require("node:url");

const cases = [
  ["\u0000", "%00"],
  ["\u001f", "%1F"],
  ["\u007f", "%7F"],
  ["\u0080", "%C2%80"],
  ["\u07ff", "%DF%BF"],
  ["\u0800", "%E0%A0%80"],
  ["\uffff", "%EF%BF%BF"],
];
for (const [character, encoded] of cases) {
  assert.strictEqual(
    url.pathToFileURL(`/${character}`).href,
    `file:///${encoded}`,
  );
}
console.log("UTF-16 boundary matrix passed");
