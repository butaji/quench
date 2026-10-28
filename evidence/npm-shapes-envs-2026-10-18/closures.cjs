const mode = process.argv[2], keep = [];
const m = { x: 1 };
function plain(m, k) { return function () { return m[k]; }; }
const arrowWrap = (m, k) => function () { return m[k]; };
function plainArrow(m, k) { return () => m[k]; }
const N = 216000;
if (mode === 'plain') for (var i = 0; i < N; i++) keep.push(plain(m, 'x'));
if (mode === 'arrowWrap') for (var i = 0; i < N; i++) keep.push(arrowWrap(m, 'x'));
if (mode === 'plainArrow') for (var i = 0; i < N; i++) keep.push(plainArrow(m, 'x'));
if (mode === 'plainLocal') { const ks = 'x'; for (var i = 0; i < N; i++) keep.push(plain(m, ks)); }
console.log(keep.length);
