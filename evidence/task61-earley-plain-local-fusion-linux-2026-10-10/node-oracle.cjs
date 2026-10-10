function localRound(seed) {
  let value = seed;
  let assigned = (value = value + 3);
  let reread = value;
  value = assigned + reread;
  return [assigned, reread, value];
}
function edgeCases() {
  let x = 4, y = -0;
  let a = (x = 9); let b = (x = x + 1); let c = (y = 2);
  return [a,b,c,x,Object.is(y,-0)];
}
function closures(seed) {
  let x = seed;
  const get = () => x;
  x = x + 2;
  const before = get();
  x = before * 3;
  return [before,get()];
}
function loop(n) { let sum=0; for (let i=0;i<n;i++) { sum = sum + i; } return sum; }
console.log(JSON.stringify([localRound(7), localRound(-8), edgeCases(), closures(5), loop(30)]));
