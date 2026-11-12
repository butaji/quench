// C: for (i = 0; i < n; i++) { s += i; if (i == -1) s = 0; }   (the handler is a never-taken branch)
function kernel(n) {
  n = n | 0;
  let s = 0;
  for (let i = 0; i < n; i = (i + 1) | 0) {
    try {
      s = (s + i) | 0;
      if (i === -1) throw new Error("never");
    } catch (e) {
      s = 0;
    }
  }
  return s;
}
console.log(kernel(5000000));
