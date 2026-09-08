function addChain(a, b, c) {
  return (a + b) + c;
}

function run() {
  return addChain("x", 2, 3);
}
function verify(result) {
if (result !== "x23") {
  throw new Error("string fallback assertion failed: " + result);
}
  return result;
}
return { run: run, verify: verify };
