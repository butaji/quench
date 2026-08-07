const assert = require("assert");
const fs = require("fs");

const source = `/tmp/quench-cp-source-${process.pid}`;
const destination = `/tmp/quench-cp-destination-${process.pid}`;
fs.mkdirSync(source);
fs.writeFileSync(`${source}/keep.js`, "ok");
fs.writeFileSync(`${source}/drop.txt`, "drop");

fs.cp(source, destination, {
  recursive: true,
  filter: async (path) => {
    await Promise.resolve();
    return fs.statSync(path).isDirectory() || path.endsWith(".js");
  },
}, (error) => {
  assert.ifError(error);
  assert(fs.existsSync(`${destination}/keep.js`));
  assert(!fs.existsSync(`${destination}/drop.txt`));
  fs.rmSync(source, { recursive: true });
  fs.rmSync(destination, { recursive: true });
  console.log("fs cp async filter passed");
});
