// gbemu's shape: an opcode `switch` over a byte read from a typed array held on `this`.
function Cpu(n) {
  this.mem = new Uint8Array(n);
  this.a = 1; this.b = 2; this.c = 3; this.d = 4;
  for (var i = 0; i < n; i++) this.mem[i] = i & 15;
}
Cpu.prototype.run = function (n) {
  var s = 0, a = this.a, b = this.b, c = this.c, d = this.d, mem = this.mem;
  for (var i = 0; i < n; i++) {
    switch (mem[i & 65535]) {
      case 0: s = (s + a) | 0; break;
      case 1: a = (a + 1) | 0; break;
      case 2: b = (b ^ a) | 0; break;
      case 3: c = (c + b) | 0; break;
      case 4: d = (d - c) | 0; break;
      case 5: a = (a << 1) | 0; break;
      case 6: b = (b >> 1) | 0; break;
      case 7: s = (s ^ d) | 0; break;
      case 8: c = (c & a) | 0; break;
      case 9: d = (d | b) | 0; break;
      case 10: a = (a + c) | 0; break;
      case 11: b = (b - d) | 0; break;
      case 12: s = (s + 1) | 0; break;
      case 13: c = (c ^ d) | 0; break;
      case 14: d = (d + a) | 0; break;
      default: s = (s - 1) | 0; break;
    }
  }
  this.a = a; this.b = b; this.c = c; this.d = d;
  return s;
};
var cpu = new Cpu(65536);
var N = 2000000;
var t0 = performance.now();
var r = cpu.run(N);
var t1 = performance.now();
console.log("switch_mem " + ((t1 - t0) * 1e6 / N).toFixed(3) + " ns/op " + r);
