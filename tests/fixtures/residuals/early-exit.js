// C: while (i < n) { if (i * i > limit) break; i++; }
function kernel(n, limit) {
  n = n | 0;
  let i = 0;
  while (i < n) {
    if (((i * i) | 0) > limit) break;
    i = (i + 1) | 0;
  }
  return i;
}
console.log(kernel(5000000, 2147483647));
