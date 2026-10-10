function recursive(n, tag) { if (n > 0) return recursive(n - 1, tag); const a = recursive.arguments; return [a.length, a[0], a[1], recursive.caller === recursive]; }
function sloppyExtra() { const a = sloppyExtra.arguments; return [a.length, a[0], a[1]]; }
function strictFn() { "use strict"; return strictFn.arguments; }
const arrowFn = () => arrowFn.arguments;
class C { constructor() { return C.arguments; } method() { return this.method.arguments; } }
function underWith() { var f; with ({ marker: 1 }) { f = function(x) { const a = f.arguments; return [a.length, a[0], a[1]]; }; } return f(7, 8); }
function outcome(fn) { try { return {value: fn()}; } catch (e) { return {error: e.name}; } }
console.log(JSON.stringify({recursive: recursive(2, "outer"), sloppyExtra: sloppyExtra(5, 6), strict: outcome(strictFn), arrow: outcome(arrowFn), classCtor: outcome(() => new C(3)), classMethod: outcome(() => new C().method(4)), nativeCaller: outcome(() => Array.prototype.push.caller), nativeArguments: outcome(() => Array.prototype.push.arguments), underWith: underWith()}));
