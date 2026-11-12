// C: for (i = 0; i < n; i++) s += a[i] * b[i];
function kernel(a, b, n) {
  let s = 0.5;
  for (let i = 0; i < n; i++) s += a[i] * b[i];
  return s;
}
const size = 1 << 16;
const a = [], b = [];
for (let i = 0; i < size; i++) { a.push(i * 0.5); b.push(i * 0.25); }
let total = 0;
for (let r = 0; r < 64; r++) total += kernel(a, b, size);
console.log(total);
