const assert = require('assert');
const { execFile } = require('child_process');

execFile(process.execPath, ['-e', 'console.log("a".repeat(1024 * 1024))'], (error) => {
  assert(error instanceof RangeError);
  assert.strictEqual(error.code, 'ERR_CHILD_PROCESS_STDIO_MAXBUFFER');
  assert.strictEqual(error.message, 'stdout maxBuffer length exceeded');
});

execFile(process.execPath, ['-e', 'console.error("中文测试");'], { maxBuffer: 10 }, (error) => {
  assert(error instanceof RangeError);
  assert.strictEqual(error.code, 'ERR_CHILD_PROCESS_STDIO_MAXBUFFER');
  assert.strictEqual(error.message, 'stderr maxBuffer length exceeded');
});
