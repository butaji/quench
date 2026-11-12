// C: for (i = 0; i < n; i++) s += (ps[i & 1].tag ? ps[i & 1].u.x : ps[i & 1].v.x);  (two layouts)
function kernel(ps, n) {
  n = n | 0;
  let s = 0;
  for (let i = 0; i < n; i = (i + 1) | 0) s = (s + ps[i & 1].x) | 0;
  return s;
}
console.log(kernel([{ x: 1, y: 2 }, { y: 2, x: 3 }], 5000000));
