function addChain(a, b, c) {
  return (a + b) + c;
}

function verify(result) {
if (result !== "x23") {
  throw new Error("string fallback assertion failed: " + result);
}
  return result;
}
return { run: addChain, arguments: ["x", 2, 3], verify: verify };
