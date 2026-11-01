function scalarArithmetic(x, y) { return ((x + y) * (x - y)) / 3; }
function scalarCall(x, y) { return scalarArithmetic(x, y); }
function scalarScale(x) { const doubled = x * 2; return doubled * 3; }
function scalarUpdate(x) { const before = x++; return [before, x]; }
function scalarOverflow() { return (2147483647 + 1) * 2; }

if (scalarCall(5, 2) !== 7 || scalarCall('5', 2) !== 52) throw 'scalar arithmetic/coercion';
const scalarNaN = scalarCall(NaN, 2);
if (scalarNaN === scalarNaN) throw 'scalar NaN';
if (!Object.is(scalarScale(-0), -0) || scalarScale(Infinity) !== Infinity
    || scalarScale(-Infinity) !== -Infinity) throw 'scalar Number representation';
if (scalarOverflow() !== 4294967296) throw 'scalar overflow continuation';
for (const value of [2147483647, -2147483648, 1.5, -0, Infinity, -Infinity, NaN]) {
    const pair = scalarUpdate(value);
    if (!Object.is(pair[0], value) || !Object.is(pair[1], value + 1)) throw 'scalar update';
}
const scalarBigInt = scalarUpdate(4n);
if (scalarBigInt[0] !== 4n || scalarBigInt[1] !== 5n) throw 'scalar BigInt fallback';
let scalarConversions = 0;
const scalarObject = { valueOf() { return ++scalarConversions; } };
if (scalarCall(scalarObject, 2) !== 0 || scalarConversions !== 2) throw 'scalar conversion order';
const scalarMarker = {};
scalarConversions = 0;
const scalarThrower = { valueOf() { if (++scalarConversions === 2) throw scalarMarker; return 1; } };
try { scalarCall(scalarThrower, 2); throw 'scalar missing throw'; }
catch (error) { if (error !== scalarMarker || scalarConversions !== 2) throw 'scalar throw order'; }
