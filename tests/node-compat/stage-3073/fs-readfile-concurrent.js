"use strict";

const assert = require("assert");
const fs = require("fs");

const files = [1024, 64 * 1024, 64 * 1024 - 1, 256 * 1024, 256 * 1024 + 1].map(
  (length, index) => {
    const path = `/tmp/quench-stage-readfile-${index}.txt`;
    const value = Buffer.alloc(length, index + 1);
    fs.writeFileSync(path, value);
    return [path, value];
  }
);

Promise.all(
  files.map(
    ([path, expected]) =>
      new Promise((resolve, reject) => {
        fs.readFile(path, (error, value) => {
          if (error) reject(error);
          else {
            assert.deepStrictEqual(value, expected);
            resolve();
          }
        });
      })
  )
).then(() => files.forEach(([path]) => fs.unlinkSync(path)));
