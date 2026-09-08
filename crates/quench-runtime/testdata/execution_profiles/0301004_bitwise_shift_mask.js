function mix(value, shift) {
  return ((value << (shift & 31)) ^ (value >>> 3)) | 0;
}

mix(1, 1);
var result = mix(-123456789, 37);
if (result !== 194173757) {
  throw new Error("bitwise shift/mask mismatch: " + result);
}
return result;
