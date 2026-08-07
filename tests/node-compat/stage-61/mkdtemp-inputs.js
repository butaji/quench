const assert = require("assert");
const fs = require("fs");
const path = require("path");
const tmp = "/tmp/quench-node-stage-61-";
const cases = [
  ["string", tmp],
  ["url", new URL(`file://${tmp}`)],
  ["buffer", Buffer.from(tmp)],
  ["uint8", new TextEncoder().encode(tmp)],
];
for (const [name, input] of cases) {
  console.log(`stage-61 ${name}`);
  const folder = fs.mkdtempSync(input);
  assert.strictEqual(
    path.basename(folder).length,
    path.basename(tmp).length + 6,
  );
  assert.strictEqual(fs.existsSync(folder), true);
  fs.rmdirSync(folder);
}
