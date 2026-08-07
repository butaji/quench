const assert = require("node:assert");
const child = require("node:child_process");

child.exec(
  "${EXEC_ENCODING_FIXTURE}",
  {
    encoding: "utf8",
    env: { EXEC_ENCODING_FIXTURE: "test-child-process-exec-encoding" },
  },
  (error, stdout, stderr) => {
    assert.strictEqual(error, null);
    assert.strictEqual(stdout, "foo\n");
    assert.strictEqual(stderr, "bar\n");
    console.log("child process exec encoding passed");
  },
);
