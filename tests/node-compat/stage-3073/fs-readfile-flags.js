"use strict";

const assert = require("assert");
const fs = require("fs");
const path = "/tmp/quench-stage-readfile-flags.txt";

fs.writeFileSync(path, "abc");
assert.strictEqual(fs.readFileSync(path, { flag: "a+" }).toString(), "abc");
assert.throws(() => fs.readFileSync(path, { flag: "ax+" }), { code: "EEXIST" });
assert.strictEqual(fs.readFileSync(path, { flag: "w+" }).length, 0);
assert.strictEqual(fs.readFileSync(path).length, 0);

fs.unlinkSync(path);
assert.strictEqual(fs.readFileSync(path, { flag: "a+" }).length, 0);
assert.throws(() => fs.readFileSync(path, { flag: "ax+" }), { code: "EEXIST" });

const asyncRead = (options) =>
  new Promise((resolve, reject) => {
    fs.readFile(path, options, (error, value) =>
      error ? reject(error) : resolve(value)
    );
  });

asyncRead({ flag: "a+", encoding: "utf8" })
  .then((value) => assert.strictEqual(value, ""))
  .then(() => assert.rejects(asyncRead({ flag: "ax+" }), { code: "EEXIST" }));
