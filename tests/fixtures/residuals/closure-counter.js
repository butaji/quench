// C: struct { int c; } env; static inline int next(env *e) { return ++e->c; }  for (...) s += next(&env);
function counter() { let c = 0; return () => { c = (c + 1) | 0; return c; }; }
function kernel(n) {
  n = n | 0;
  const next = counter();
  let s = 0;
  for (let i = 0; i < n; i = (i + 1) | 0) s = (s + next()) | 0;
  return s;
}
console.log(kernel(5000000));
