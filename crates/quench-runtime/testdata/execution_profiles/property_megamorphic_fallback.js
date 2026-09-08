function read(object) {
  return object.value;
}

var a = { value: 10 };
var b = { prefix: 1, value: 11 };
var c = { first: 1, second: 2, value: 12 };
read(a);
read(b);
read(c);
var result = read(c);
if (result !== 12) {
  throw new Error("megamorphic property fallback mismatch");
}
return result;
