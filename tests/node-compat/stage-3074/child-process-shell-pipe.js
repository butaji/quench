"use strict";

const assert = require("assert");
const fs = require("fs");
const { exec } = require("child_process");

if (process.argv[2] === "child") {
  fs.readFile("/dev/stdin", (error, data) => {
    if (error) throw error;
    process.stdout.write(data);
  });
} else {
  const input = "/tmp/quench-stage-shell-pipe.txt";
  const expected = "shell pipe\n";
  fs.writeFileSync(input, expected);
  exec(
    `"${process.execPath}" "${__filename}" child < "${input}"`,
    (error, stdout, stderr) => {
      assert.ifError(error);
      assert.strictEqual(stdout, expected);
      assert.strictEqual(stderr, "");
      fs.unlinkSync(input);
      exec(`"${process.execPath}" "${__filename}.missing"`, (error, stdout) => {
        assert(error);
        assert.strictEqual(stdout, "");
      });
    }
  );
}
