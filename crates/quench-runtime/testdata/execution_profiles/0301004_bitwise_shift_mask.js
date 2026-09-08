function mix(value, shift) {
  return ((value << (shift & 31)) ^ (value >>> 3)) | 0;
}

mix(1, 1);
function verify(result) {
if (result !== 194173757) {
  throw new Error("bitwise shift/mask mismatch: " + result);
}
  return result;
}
return { run: mix, arguments: [-123456789, 37], verify: verify };
