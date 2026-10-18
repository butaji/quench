const mode = process.argv[2], N = 80, K = 2700;
const keys = []; for (let i = 0; i < K; i++) keys.push('export' + i);
const m = {}; for (const k of keys) m[k] = 1;
const keep = [];
function shared() { return 1; }
for (let j = 0; j < N; j++) {
  const o = {};
  for (const k of keys) {
    if (mode === 'none') continue;
    else if (mode === 'data') o[k] = 1;
    else if (mode === 'sharedget') Object.defineProperty(o, k, { enumerable: true, get: shared });
    else if (mode === 'arrow') ((m, k) => Object.defineProperty(o, k, { enumerable: true, get: () => m[k] }))(m, k);
    else if (mode === 'function') ((m, k) => Object.defineProperty(o, k, { enumerable: true, get: function () { return m[k]; } }))(m, k);
    else if (mode === 'closures') keep.push(((m, k) => function () { return m[k]; })(m, k));
    else if (mode === 'arrowclosures') keep.push(((m, k) => () => m[k])(m, k));
  }
  keep.push(o);
}
console.log(keep.length);
