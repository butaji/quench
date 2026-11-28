// A CommonJS entry whose prototype methods are made once, at the top of the module, before any call: each closure is one
// object, so compiled callers call it directly. Legacy reflection never hands out the loader's wrapper.
function S(n) { this.n = n; }
S.prototype.a = function () { return this.n + 1; };
S.prototype.go = function () { var t = 0; for (var i = 0; i < 1000; i++) t = (t + this.a()) | 0; return t; };
function f() { return f.caller; }
var s = new S(1);
var r = 0;
for (var k = 0; k < 3000; k++) r = (r + s.go()) | 0;
console.log("sum " + r + " caller " + f());
