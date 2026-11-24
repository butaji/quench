// A global function folded as a constant: reassigning the global retires the code.
var helper = function (x) { return x + 1; };
function run(n) { let s = 0; for (let i = 0; i < n; i++) s = helper(s); return s; }
let s = 0;
for (let k = 0; k < 200; k++) s += run(100);
helper = function (x) { return x + 2; };
const after = run(100);
if (s !== 20000 || after !== 200) throw new Error("global cell: " + s + " " + after);
console.log("ok global-cell");
