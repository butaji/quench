function target() {
  return 42;
}

function invoke(fn) {
  return fn();
}

invoke(target);
var result = invoke(target);
if (result !== 42) {
  throw new Error("direct zero-argument call mismatch");
}
return result;
