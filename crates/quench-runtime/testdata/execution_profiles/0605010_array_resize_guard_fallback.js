function readAfterResize(values) {
  values.length = 1;
  return values[2];
}

function run() {
  return readAfterResize([10, 20, 30]);
}
function verify(result) {
if (result !== undefined) {
  throw new Error("array resize guard fallback mismatch");
}
  return result;
}
return { run: run, verify: verify };
