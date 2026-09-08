function addChain(a, b, c) {
  return (a + b) + c;
}

function verify(result) {
if (result !== 7) {
  throw new Error("add-chain assertion failed: " + result);
}
  return result;
}
return { run: addChain, arguments: [1, 2, 4], verify: verify };
