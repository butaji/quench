// Reads through String.prototype and Number.prototype hints.
function sl(s) { return s.twice(); }
function nl(n) { return n.half(); }
String.prototype.twice = function () { return this + this; };
Number.prototype.half = function () { return this / 2; };
let a = 0, b = 0;
for (let k = 0; k < 20000; k++) { a += sl("ab").length; b += nl(4); }
String.prototype.twice = function () { return "z"; };
Number.prototype.half = function () { return -1; };
if (a !== 80000 || b !== 40000 || sl("ab") !== "z" || nl(4) !== -1) throw new Error("primitive prototypes");
console.log("ok primitive-prototypes");
