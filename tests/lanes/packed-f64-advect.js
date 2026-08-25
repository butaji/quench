const width = 4096;
const height = 64;
const rowSize = width + 2;
const cells = rowSize * (height + 2);
const d = new Array(cells).fill(0);
const d0 = new Array(cells).fill(1.25);
const u = new Array(cells).fill(0.001);
const v = new Array(cells).fill(0.001);

function advect(d, d0, u, v, dt) {
  var Wdt0 = dt * width;
  var Hdt0 = dt * height;
  var Wp5 = width + 0.5;
  var Hp5 = height + 0.5;
  for (var j = 1; j <= height; j++) {
    var pos = j * rowSize;
    for (var i = 1; i <= width; i++) {
      var x = i - Wdt0 * u[++pos];
      var y = j - Hdt0 * v[pos];
      if (x < 0.5) x = 0.5;
      else if (x > Wp5) x = Wp5;
      var i0 = x | 0;
      var i1 = i0 + 1;
      if (y < 0.5) y = 0.5;
      else if (y > Hp5) y = Hp5;
      var j0 = y | 0;
      var j1 = j0 + 1;
      var s1 = x - i0;
      var s0 = 1 - s1;
      var t1 = y - j0;
      var t0 = 1 - t1;
      var row1 = j0 * rowSize;
      var row2 = j1 * rowSize;
      d[pos] =
        s0 * (t0 * d0[i0 + row1] + t1 * d0[i0 + row2]) +
        s1 * (t0 * d0[i1 + row1] + t1 * d0[i1 + row2]);
    }
  }
}

for (let round = 0; round < 100; round++) advect(d, d0, u, v, 0.001);
if (!(d[rowSize + 1] > 0)) throw new Error("advect result");
