const assert = require("assert");
const fs = require("fs");

(async () => {
  const path = `/tmp/quench-opendir-${process.pid}`;
  fs.mkdirSync(path);
  fs.writeFileSync(`${path}/entry`, "x");
  const names = [];
  for await (const entry of await fs.promises.opendir(path)) {
    names.push(entry.name);
  }
  assert.deepStrictEqual(names, ["entry"]);
  fs.rmSync(path, { recursive: true });
  console.log("fs opendir async iterator passed");
})();
