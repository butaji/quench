function addChain(a, b, c) {
  return (a + b) + c;
}

function run() {
  return addChain(1, 2, 4);
}
function verify(result) {
if (result !== 7) {
  throw new Error("add-chain assertion failed: " + result);
}
  return result;
}
return { run: run, verify: verify };
