function read(object) {
  return object.missing;
}

read({ value: 1 });
var result = read({ value: 2 });
if (result !== undefined) {
  throw new Error("missing-property fallback mismatch");
}
return result;
