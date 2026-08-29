"use strict";

const common = require("../../tests/node/test/common");
const fs = require("fs");

new Promise((resolve, reject) => {
  fs.readFile(
    "tests/node/test/fixtures/x.txt",
    common.mustCall((error, value) => {
      if (error) reject(error);
      else resolve(value);
    })
  );
}).then(
  common.mustCall((value) => {
    if (value.toString() !== "xyz\n") throw new Error("unexpected content");
  })
);
