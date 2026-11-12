// C: static inline int add(int a, int b) { return a + b; }  for (...) s = add(s, i);
function add(a, b) { return (a + b) | 0; }
function kernel(n) {
  n = n | 0;
  let s = 0;
  for (let i = 0; i < n; i = (i + 1) | 0) s = add(s, i);
  return s;
}
console.log(kernel(5000000));
