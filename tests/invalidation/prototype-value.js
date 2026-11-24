// A method read through a prototype hint: replacing the prototype's value retires the code.
function P() { this.x = 1; }
P.prototype.get = function () { return this.x; };
function run(p) { let s = 0; for (let i = 0; i < 100; i++) s += p.get(); return s; }
const p = new P();
let s = 0;
for (let k = 0; k < 300; k++) s += run(p);
P.prototype.get = function () { return 2; };
const after = run(p);
if (s !== 30000 || after !== 200) throw new Error("prototype value: " + s + " " + after);
console.log("ok prototype-value");
