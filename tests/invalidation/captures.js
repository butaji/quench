// A captured binding the code folded is reassigned by another closure.
function outer() {
  let f = (x) => x + 1;
  function run() { let s = 0; for (let i = 0; i < 100; i++) s = f(s); return s; }
  function swap() { f = (x) => x + 3; }
  return { run, swap };
}
const o = outer();
let s = 0;
for (let k = 0; k < 300; k++) s += o.run();
o.swap();
const after = o.run();
if (s !== 30000 || after !== 300) throw new Error("captures: " + s + " " + after);
console.log("ok captures");
