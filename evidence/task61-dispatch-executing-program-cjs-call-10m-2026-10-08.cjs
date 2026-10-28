function callee(value) { return value + 1; }
function loop(fn) {
  var value = 0;
  for (var index = 0; index < 10000000; index++) value = fn(index);
  if (value !== 10000000) throw new Error('bad call loop');
}
loop(callee);
