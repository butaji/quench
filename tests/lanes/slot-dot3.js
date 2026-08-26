function Left(a, b, c) {
  this.a = a;
  this.b = b;
  this.c = c;
}

function Right(u, v, w) {
  this.u = u;
  this.v = v;
  this.w = w;
}

Left.prototype.project = function (other) {
  return this.a * other.u + this.b * other.v + this.c * other.w;
};

var left = new Left(2, 3, 4);
var right = new Right(5, 6, 7);
var checksum = 0;
for (var iteration = 0; iteration < 300000; iteration++) {
  checksum += left.project(right);
}
console.log(checksum);
