function read(object) {
  return object.missing;
}

read({ value: 1 });
function run() {
  return read({ value: 2 });
}
function verify(result) {
if (result !== undefined) {
  throw new Error("missing-property fallback mismatch");
}
  return result;
}
return { run: run, verify: verify };
