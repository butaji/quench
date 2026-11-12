function make(delta) {
    let step = function step(state) {
        state.count = (state.count - 1) | 0;
        state.value = state.value + delta;
    };
    function run(seed) {
        const state = {count: 20, value: seed};
        globalThis.last = state;
        while (state.count > 0) step(state);
        return state.value;
    }
    return {run, replace(value) {step = value;}};
}
const a = make(2), b = make(5);
if (a.run(3) !== 43 || b.run(3) !== 103) throw 'closure environment';
a.replace(function replacement(state) {state.count--; state.value += 100;});
if (a.run(3) !== 2003 || b.run(3) !== 103) throw 'replacement';
let calls = 0;
const marker = {};
const failing = make({valueOf() {calls++; throw marker;}});
try {failing.run(3); throw 'missing throw';} catch (error) {if (error !== marker) throw error;}
if (calls !== 1 || last.count !== 19 || last.value !== 3) throw 'replayed prefix';
throw 42;
