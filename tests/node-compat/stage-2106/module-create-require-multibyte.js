const assert = require("assert");
const { createRequire } = require("module");

const encoded =
  `file://${process.cwd()}/tests/node/test/fixtures/copy/utf/%E6%96%B0%E5%BB%BA%E6%96%87%E4%BB%B6%E5%A4%B9/index.js`;
assert.deepStrictEqual(createRequire(encoded)("./experimental"), {
  ofLife: 42,
});
console.log("module createRequire multibyte passed");
