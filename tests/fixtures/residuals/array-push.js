// C: for (i = 0; i < n; i++) vec_push(&v, i);   (amortized growth stays in the cold path)
function kernel(n) {
  n = n | 0;
  const a = [];
  for (let i = 0; i < n; i = (i + 1) | 0) a.push(i);
  return a.length;
}
let length = 0;
for (let r = 0; r < 8; r++) length += kernel(1 << 20);
console.log(length);
