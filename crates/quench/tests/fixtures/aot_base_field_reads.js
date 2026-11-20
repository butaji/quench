// AOT base bodies read fields of polymorphic sites and narrow elements: a base body's narrow decode
// table and its poll's attention byte are the running process's, not the compiling one's.
function B(n) { this.array = new Array(); for (var i = 0; i < n; i++) this.array[i] = (i * 2654435761) & 0xfffffff; this.t = n; }
function C() { this.array = [1, 2, 3]; this.t = 3; }
function read(o) { var a = o.array; var s = 0; for (var i = 0; i < 3; i++) s = s + i; return typeof a + s; }
function am3(i, x, w, j, c, n) {
  var this_array = this.array, w_array = w.array;
  var xl = x & 0x3fff, xh = x >> 14;
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
B.prototype.am = am3;
var seen = {}, hash = 0;
for (var k = 0; k < 3000; k++) {
  var b = new B(4), c = new C(), lit = { array: [1], t: 1 };
  var r = read(b) + read(c) + read(lit);
  if (!seen[r]) { seen[r] = 1; console.log(k, r); }
  var x = new B(8), w = new B(16);
  hash = (hash * 31 + x.am(0, 0x0abcdef + k, w, k & 7, 0, 8) + w.array[(k & 7) + 3]) | 0;
}
console.log(hash);
