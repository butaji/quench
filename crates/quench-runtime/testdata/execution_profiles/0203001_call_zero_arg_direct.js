function target() {
  return 42;
}

function invoke(fn) {
  return fn();
}

invoke(target);
function verify(result) {
if (result !== 42) {
  throw new Error("direct zero-argument call mismatch");
}
  return result;
}
return { run: invoke, arguments: [target], verify: verify };
