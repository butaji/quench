function returnUndefined() { return undefined; }
function returnNull() { return null; }
function returnBoolean() { return true; }
function returnInteger() { return 42; }
function returnDouble() { return 1.25; }
function returnNaN() { return NaN; }
function returnInfinity() { return Infinity; }
function returnString() { return 'constant return'; }
function returnFinally() { try { return 17; } finally {} }
function returnCatch() { try { throw 5; } catch (_) { return 19; } }
function Constructor() { return 7; }
const arrow = () => 'arrow return';
const value = [returnUndefined(), returnNull(), returnBoolean(), returnInteger(), returnDouble(), String(returnNaN()), returnInfinity(), returnString(), returnFinally(), returnCatch(), arrow(), new Constructor() instanceof Constructor, new Constructor().valueOf()];
console.log(JSON.stringify(value));
