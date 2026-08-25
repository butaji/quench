const limbs = 256;
const rounds = 100000;

function BigInteger(array) {
  this.array = array;
}

function am3(i, x, w, j, c, n) {
  var this_array = this.array;
  var w_array = w.array;
  var xl = x & 0x3fff,
    xh = x >> 14;
  while (--n >= 0) {
    var l = this_array[i] & 0x3fff;
    var h = this_array[i++] >> 14;
    var m = xh * l + h * xl;
    l = xl * l + ((m & 0x3fff) << 14) + w_array[j] + c;
    c = (l >> 28) + (m >> 14) + xh * h;
    w_array[j++] = l & 0xfffffff;
  }
  return c;
}

BigInteger.prototype.am = am3;
const inputArray = [];
const outputArray = [];
for (let i = 0; i < limbs; i++) {
  inputArray[i] = 1234567;
  outputArray[i] = 7654321;
}
const input = new BigInteger(inputArray);
const output = new BigInteger(outputArray);
let checksum = 0;
for (let round = 0; round < rounds; round++) {
  checksum ^= input.am(0, 0x1234567, output, 0, round & 255, limbs);
}
if (!Number.isFinite(checksum) || output.array[limbs - 1] < 0)
  throw new Error("am3 result");
