const assert = require("assert");
const { execFile } = require("child_process");

const command = process.execPath;
const args = [];
const options = {};
const callback = () => {};

execFile(command);
execFile(command, args, options, callback);
execFile(command, callback, "legacy-placeholder");

for (const call of [
  () => execFile(command, "bad-args"),
  () => execFile(command, args, "bad-options"),
  () => execFile(command, options, "bad-callback"),
  () => execFile(command, args, args)
]) {
  assert.throws(call, { code: "ERR_INVALID_ARG_TYPE", name: "TypeError" });
}
