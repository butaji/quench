// C: the producer and consumer fused into one loop: for (i = 0; i < n; i++) s += i;
function* produce(n) { for (let i = 0; i < n; i = (i + 1) | 0) yield i; }
function kernel(n) {
  let s = 0;
  for (const v of produce(n)) s = (s + v) | 0;
  return s;
}
console.log(kernel(2000000));
