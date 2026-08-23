// child_process.spawnSync — real subprocess execution. These spawn
// real OS binaries (no self-re-exec), so they verify genuine child
// stdout capture, status codes, and env passthrough under any host.
'use strict';
const assert = require('assert');
const cp = require('child_process');

// Capture the child's stdout and pid.
const echo = cp.spawnSync('/bin/echo', ['hello', 'world']);
assert.strictEqual(echo.status, 0, `echo status`);
assert.ok(echo.pid > 0, 'echo pid');
assert.strictEqual(echo.stdout.trim(), 'hello world', 'echo stdout');

// A non-zero exit status is surfaced via `status`, not thrown.
const fail = cp.spawnSync('/bin/sh', ['-c', 'exit 1']);
assert.strictEqual(fail.status, 1, 'sh -c exit 1 status');

// Missing command returns status:null with a coded error (no throw).
const missing = cp.spawnSync('/definitely/not/a/command');
assert.strictEqual(missing.status, null, 'missing status');
assert.ok(missing.error, 'missing error');
assert.strictEqual(missing.error.code, 'ENOENT', 'missing code');

// The inherited environment flows to the child.
const env = cp.spawnSync('/bin/sh', ['-c', 'echo -n "$HOME"']);
assert.strictEqual(env.status, 0, 'env status');
assert.ok(env.stdout.length > 0, 'HOME passed through');

console.log('child_process: ok');