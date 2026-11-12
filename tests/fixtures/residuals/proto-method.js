// C: static inline void inc(P *p, int d) { p->v += d; }  for (...) inc(&p, i);
class Counter {
  constructor() { this.v = 0; }
  inc(d) { this.v = (this.v + d) | 0; }
}
function kernel(p, n) {
  n = n | 0;
  for (let i = 0; i < n; i = (i + 1) | 0) p.inc(i);
  return p.v;
}
console.log(kernel(new Counter(), 5000000));
