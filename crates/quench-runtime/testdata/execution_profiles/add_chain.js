function addChain(a, b, c) {
  return (a + b) + c;
}

var result = addChain(1, 2, 4);
if (result !== 7) {
  throw new Error("add-chain assertion failed: " + result);
}
return result;
