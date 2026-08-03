const assert = require("assert");
const fs = require("fs");
const folder = fs.mkdtempSync("/tmp/quench-node-");
const nested = `${folder}/nested`;
fs.promises
  .mkdir(nested)
  .then(() => fs.promises.readdir(folder))
  .then((entries) => {
    assert.deepStrictEqual(entries, ["nested"]);
    return fs.promises.stat(nested);
  })
  .then((stat) => {
    assert.strictEqual(stat.isDirectory(), true);
    fs.rmdirSync(folder);
  });
