function dispatch(kind, value) {
  switch (kind) {
    case 0: return value + 1;
    case 1: return value * 2;
    case 2: return value - 3;
    default: return -1;
  }
}

dispatch(0, 1);
function run() {
  return dispatch(1, 21) + dispatch(2, 10);
}
function verify(result) {
if (result !== 49) {
  throw new Error("integer switch dispatch mismatch");
}
  return result;
}
return { run: run, verify: verify };
