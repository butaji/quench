// C: for (i) for (j) { s = 0; for (k) s += a[i*n+k] * b[k*n+j]; c[i*n+j] = s; }
function kernel(a, b, c, n) {
  for (let i = 0; i < n; i++) {
    for (let j = 0; j < n; j++) {
      let s = 0;
      for (let k = 0; k < n; k++) s += a[i * n + k] * b[k * n + j];
      c[i * n + j] = s;
    }
  }
}
const n = 96, a = [], b = [], c = [];
for (let i = 0; i < n * n; i++) { a.push(i % 7 + 0.5); b.push(i % 5 + 0.25); c.push(0); }
for (let r = 0; r < 4; r++) kernel(a, b, c, n);
console.log(c[n * n - 1]);
