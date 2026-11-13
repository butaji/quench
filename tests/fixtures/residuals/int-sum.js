// C: for (int i = 0; i < n; i++) s += i;
function kernel(n) {
  n = n | 0;
  let s = 0;
  for (let i = 0; i < n; i = (i + 1) | 0) s = (s + i) | 0;
  return s;
}
console.log(kernel(5000000));
