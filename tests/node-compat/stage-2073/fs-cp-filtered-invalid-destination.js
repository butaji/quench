const assert = require("assert");
const fs = require("fs");
const path = require("path");

const root = path.join(process.cwd(), "tests/node/test/.tmp.0/cp-filtered");
const source = path.join(root, "bar");
const destination = path.join(root, "dest", "bar");
fs.mkdirSync(source, { recursive: true });
fs.mkdirSync(path.dirname(destination), { recursive: true });
fs.writeFileSync(destination, "existing");
const options = { recursive: true, filter: (value) => !value.endsWith("bar") };
fs.cp(source, destination, options, (error) => {
  assert.strictEqual(error, null);
});
fs.cpSync(source, destination, options);
