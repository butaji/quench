// A method name the program stores once (X.prototype.push) is only a guess of what `a.push` calls: an
// array receiver calls Array.prototype.push. An AOT image inlined the guess without a guard (TOD-2222).
function X(){ this.n = 0; }
X.prototype.push = function (v) { return 42 + v; };
function f(a, i) { return a.push(i); }
let s = 0;
const x = new X();
for (let i = 0; i < 200000; i++) { s += f(i % 2 ? x : [], i & 7); }
console.log(s);
