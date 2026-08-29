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

(async () => {
  await new Promise((resolve, reject) => {
    fs.readFile(
      "tests/node/test/fixtures/x.txt",
      common.mustCall((error) => {
        if (error) reject(error);
        else resolve();
      })
    );
  });
})().then(common.mustCall());

async function withFstatSizeZero(fn) {
  const originalFstat = internalBinding("fs").fstat;
  internalBinding("fs").fstat = function (...args) {
    const stats = Reflect.apply(originalFstat, this, args);
    if (stats !== undefined) stats[8] = 0;
    return stats;
  };
  try {
    await fn();
  } finally {
    internalBinding("fs").fstat = originalFstat;
  }
}

(async () => {
  await withFstatSizeZero(
    common.mustCall(async () => {
      await new Promise((resolve, reject) => {
        fs.readFile(
          "tests/node/test/fixtures/x.txt",
          common.mustCall((error) => (error ? reject(error) : resolve()))
        );
      });
    })
  );
})().then(common.mustCall());
