function addChain(a, b, c) {
  return (a + b) + c;
}

var result = addChain("x", 2, 3);
if (result !== "x23") {
  throw new Error("string fallback assertion failed: " + result);
}
return result;
