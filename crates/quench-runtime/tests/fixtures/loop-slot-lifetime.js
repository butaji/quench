function advance(s, value) {
    'use strict';
    s.count = (s.count - 1) | 0;
    s.value = value;
}
function run(s, value) {
    while (s.count > 0) advance(s, value);
}
for (let i = 0; i < 64; i++) run({count: 3, value: 0}, i);

// The prefix count write must commit once before a sibling widens storage.
for (const value of [0.5, 2147483647, {child: 7}]) {
    const s = {count: 3, value: 0};
    run(s, value);
    if (s.count !== 0 || s.value !== value) throw 'widening';
    // Reenter the same code with the same object, now in arena storage.
    s.count = 3;
    run(s, value);
    if (s.count !== 0 || s.value !== value) throw 'arena reentry';
}

const marker = {};
let s = {count: 3, value: 0};
run(s, 1);
s.count = 3;
Object.defineProperty(s, 'value', {set(v) {throw marker;}, configurable: true});
try { run(s, 2); throw 'missing throw'; } catch (e) {if (e !== marker) throw e;}
if (s.count !== 2) throw 'replayed prefix';

s = {count: 3, value: 0};
run(s, 1);
s.count = 3;
Object.freeze(s);
try {run(s, 2); throw 'frozen write';} catch (e) {if (!(e instanceof TypeError)) throw e;}
if (s.count !== 3 || s.value !== 1) throw 'frozen effect';

s = {count: 3, value: 0};
run(s, 1);
let count = 3, gets = 0, sets = 0;
Object.defineProperty(s, 'count', {
    get() {gets++; return count;},
    set(v) {sets++; count = v;},
    configurable: true
});
run(s, 2);
if (count !== 0 || gets !== 7 || sets !== 3 || s.value !== 2) throw 'accessor reentry';
