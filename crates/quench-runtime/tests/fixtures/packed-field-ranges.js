function advancePositive(s) { s.count = (s.count - 1) | 0; }
function positive(s) { while (s.count > 0) advancePositive(s); return s.count; }
function decrement(s, n) {
    while (n > 0) { s.value = (s.value - 1) | 0; n--; }
    return s.value;
}
function staleSnapshot(s, n, replacement) {
    while (n > 0) {
        const old = s.value;
        const copy = old;
        s.value = replacement;
        if (copy > 0) s.value = (s.value - 1) | 0;
        n--;
    }
    return s.value;
}
function diamond(s, n, choose) {
    while (n > 0) {
        if (choose) s.value = 1;
        else s.value = 2;
        s.value = (s.value - 1) | 0;
        n--;
    }
    return s.value;
}
function aliased(a, b, n, replacement) {
    while (n > 0) {
        if (a.value > 0) {
            b.value = replacement;
            a.value = (a.value - 1) | 0;
        }
        n--;
    }
    return a.value;
}
for (let i = 0; i < 64; i++) {
    positive({count: 3});
    decrement({value: 3}, 2);
    staleSnapshot({value: 3}, 1, 2);
    diamond({value: 3}, 2, i & 1);
    const s = {value: 3};
    aliased(s, s, 1, 2);
}
for (const count of [-2147483648, -536870912, -1, 0, 1, 17]) {
    if (positive({count}) !== (count > 0 ? 0 : count)) throw 'positive boundary';
}
for (const start of [-2147483648, -536870912, -536870911, -1, 0, 536870911, 2147483647]) {
    const s = {value: start};
    if (decrement(s, 2) !== ((start - 2) | 0)) throw 'decrement boundary';
    s.value = start;
    if (decrement(s, 2) !== ((start - 2) | 0)) throw 'arena reentry';
    if (staleSnapshot({value: 3}, 1, start) !== ((start - 1) | 0)) throw 'stale snapshot';
    const a = {value: 3};
    if (aliased(a, a, 1, start) !== ((start - 1) | 0)) throw 'receiver alias';
}
for (const choose of [false, true]) {
    if (diamond({value: 3}, 3, choose) !== (choose ? 0 : 1)) throw 'diamond';
}

function bitwiseLoop(s, n) {
    while (n > 0) {
        s.control = (s.control >> 1) ^ ((s.control & 1) ? 0x1021 : 0);
        n--;
    }
    return s.control;
}
function variableShift(s, n, shift) {
    while (n > 0) { s.control = s.control >> shift; n--; }
    return s.control;
}
function unsignedShift(s, n) {
    while (n > 0) { s.control = s.control >>> 0; n--; }
    return s.control;
}
for (let i = 0; i < 64; i++) {
    bitwiseLoop({control: i}, 3);
    variableShift({control: i}, 2, 1);
    unsignedShift({control: i}, 2);
}
for (const seed of [-2147483648, -536870912, -1, 0, 1, 536870911, 2147483647,
        2147483648, 4294967295, 1.5, -0, NaN, Infinity, '7', null]) {
    let expected = seed;
    for (let n = 0; n < 3; n++) expected = (expected >> 1) ^ ((expected & 1) ? 0x1021 : 0);
    const state = {control: seed};
    if (bitwiseLoop(state, 3) !== expected) throw 'bitwise boundary';
    state.control = seed;
    if (bitwiseLoop(state, 3) !== expected) throw 'bitwise arena reentry';
    for (const shift of [-33, -32, -1, 0, 1, 31, 32, 33, 64]) {
        if (variableShift({control: seed}, 2, shift) !== ((seed >> shift) >> shift)) throw 'shift count';
    }
    if (unsignedShift({control: seed}, 2) !== (seed >>> 0)) throw 'unsigned shift';
}
let bitwiseCoercions = 0;
const bitwiseObject = {valueOf() { bitwiseCoercions++; return 7; }};
if (bitwiseLoop({control: bitwiseObject}, 1) !== ((7 >> 1) ^ 0x1021) || bitwiseCoercions !== 2) throw 'bitwise coercion';
const bitwiseMarker = {};
const throwingBits = {control: {valueOf() {throw bitwiseMarker;}}};
try {bitwiseLoop(throwingBits, 1); throw 'missing bitwise throw';} catch(e) {if(e !== bitwiseMarker) throw e;}
try {bitwiseLoop({control: 1n}, 1); throw 'missing bigint mismatch';} catch(e) {if(!(e instanceof TypeError)) throw e;}
