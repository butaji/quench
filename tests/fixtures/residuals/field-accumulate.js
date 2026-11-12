// C: for (i = 0; i < n; i++) o->sum += o->step;
function kernel(o, n) {
  n = n | 0;
  for (let i = 0; i < n; i = (i + 1) | 0) o.sum = (o.sum + o.step) | 0;
  return o.sum;
}
console.log(kernel({ sum: 0, step: 3 }, 5000000));
