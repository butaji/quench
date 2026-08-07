"use strict";

const assert = require("assert");
const fs = require("fs");
const path = require("path");

(async () => {
  const directory = fs.mkdtempSync(
    path.join(process.env.TMPDIR || "/tmp", "quench-glob-"),
  );
  fs.writeFileSync(path.join(directory, "a.txt"), "a");
  fs.writeFileSync(path.join(directory, "b.js"), "b");
  const matches = [];
  for await (const entry of fs.promises.glob("*.txt", { cwd: directory })) {
    matches.push(entry);
  }
  assert.deepStrictEqual(matches, [`${directory}/a.txt`]);
  console.log("fs glob passed");
})();
