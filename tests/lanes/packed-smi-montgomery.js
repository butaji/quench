const limbs = 128;
const rounds = 2000;
const BI_DV = 0x10000000;
const BI_DM = 0x0fffffff;

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
    w_array[j++] = l & 0x0fffffff;
  }
  return c;
}

function reduce(x) {
  var x_array = x.array;
  for (var i = 0; i < this.m.t; ++i) {
    var j = x_array[i] & 0x7fff;
    var u0 =
      (j * this.mpl +
        (((j * this.mph + (x_array[i] >> 15) * this.mpl) & this.um) <<
          15)) &
      BI_DM;
    j = i + this.m.t;
    x_array[j] += this.m.am(0, u0, x, i, 0, this.m.t);
    while (x_array[j] >= BI_DV) {
      x_array[j] -= BI_DV;
      x_array[++j]++;
    }
  }
}

BigInteger.prototype.am = am3;
const modulusArray = [];
const valueArray = [];
for (let i = 0; i < limbs; i++) modulusArray[i] = (1234567 + i * 101) & BI_DM;
for (let i = 0; i < 2 * limbs + 2; i++) valueArray[i] = (7654321 + i * 17) & BI_DM;
const modulus = new BigInteger(modulusArray);
const value = new BigInteger(valueArray);
const montgomery = { m: modulus, mpl: 12345, mph: 2345, um: 0x3fff, reduce };
for (let round = 0; round < rounds; round++) montgomery.reduce(value);
if (
  value.array[0] !== 0 ||
  value.array[1] !== 0 ||
  value.array[limbs - 1] !== 0 ||
  value.array[limbs] !== 57732042 ||
  value.array[2 * limbs - 1] !== 203533076
)
  throw new Error("montgomery result");
