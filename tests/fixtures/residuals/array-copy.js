// C: for (i = 0; i < n; i++) dst[i] = src[i];
function kernel(dst, src, n) {
  for (let i = 0; i < n; i++) dst[i] = src[i];
}
const size = 1 << 16;
const src = [], dst = [];
for (let i = 0; i < size; i++) { src.push(i); dst.push(0); }
for (let r = 0; r < 64; r++) kernel(dst, src, size);
console.log(dst[size - 1]);
