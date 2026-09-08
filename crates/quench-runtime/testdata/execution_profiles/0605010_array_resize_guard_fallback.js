function readAfterResize(values) {
  values.length = 1;
  return values[2];
}

var result = readAfterResize([10, 20, 30]);
if (result !== undefined) {
  throw new Error("array resize guard fallback mismatch");
}
return result;
