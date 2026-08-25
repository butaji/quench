const limbs = 128;
const rounds = 2000;
const BI_DV = 0x10000000;

function BigInteger(array) {
  this.array = array;
  this.t = limbs;
  this.s = 0;
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

function squareTo(r) {
  var x = this;
  var x_array = x.array;
  var r_array = r.array;
  var i = (r.t = 2 * x.t);
  while (--i >= 0) r_array[i] = 0;
  for (i = 0; i < x.t - 1; ++i) {
    var c = x.am(i, x_array[i], r, 2 * i, 0, 1);
    if (
      (r_array[i + x.t] += x.am(
        i + 1,
        2 * x_array[i],
        r,
        2 * i + 1,
        c,
        x.t - i - 1,
      )) >= BI_DV
    ) {
      r_array[i + x.t] -= BI_DV;
      r_array[i + x.t + 1] = 1;
    }
  }
}

BigInteger.prototype.am = am3;
BigInteger.prototype.squareTo = squareTo;
const inputArray = [];
const outputArray = [];
for (let i = 0; i < limbs; i++) inputArray[i] = 1234567;
for (let i = 0; i < 2 * limbs; i++) outputArray[i] = 0;
const input = new BigInteger(inputArray);
const output = new BigInteger(outputArray);
for (let round = 0; round < rounds; round++) input.squareTo(output);
if (
  output.array[0] !== 247593777 ||
  output.array[64] !== 256266988 ||
  output.array[127] !== 17340744 ||
  output.array[128] !== 38188101 ||
  output.array[200] !== 196202898 ||
  output.array[254] !== 11355 ||
  output.array[255] !== 0
)
  throw new Error("square result");
