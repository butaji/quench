const assert = require("assert");
const { spawn } = require("child_process");
// getTestCases(false)

if (process.argv[2] !== undefined) {
  process.exitCode = [42, 42, 0, 1, 99, 0, 97, 98, 0, 7, 6][
    Number(process.argv[2])
  ];
} else {
  const expected = [42, 42, 0, 1, 99, 0, 97, 98, 0, 7, 6];
  expected.forEach((code, index) => {
    spawn(process.execPath, [__filename, String(index)]).on(
      "exit",
      (actual) => {
        assert.strictEqual(actual, code);
      }
    );
  });
}
