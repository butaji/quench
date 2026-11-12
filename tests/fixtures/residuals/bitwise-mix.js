// C: for (i = 0; i < n; i++) { x ^= x << 13; x ^= x >> 17; x ^= x << 5; }   (xorshift32, crypto-like mixing)
function kernel(n, seed) {
  n = n | 0;
  let x = seed | 0;
  for (let i = 0; i < n; i = (i + 1) | 0) {
    x = x ^ (x << 13);
    x = x ^ (x >>> 17);
    x = x ^ (x << 5);
  }
  return x | 0;
}
console.log(kernel(5000000, 2463534242));
