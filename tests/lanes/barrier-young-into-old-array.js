// A hot element store of a young heap value into an array a collection left marked must log
// the array (staged::barrier::value_unlogged): otherwise the next collection frees the young
// value while the old array still points at it. Run in every mode, --u-jit included.
function put(a, i) { a[1] = { v: i }; a[2] = [i]; }
function lost() {
  const olds = [];
  for (let i = 0; i < 2048; i++) olds.push([0, 0, 0, 0]);
  let bad = 0;
  for (let r = 0; r < 200; r++) {
    for (let i = 0; i < olds.length; i++) put(olds[i], i + r);
    let junk = 0;
    for (let j = 0; j < 30000; j++) { const t = { j, a: [j] }; junk += t.a[0]; }
    for (let i = 0; i < olds.length; i++) {
      const a = olds[i];
      if (!a[1] || a[1].v !== i + r || a[2][0] !== i + r) bad++;
    }
  }
  return bad;
}
const bad = lost();
if (bad !== 0) throw new Error(`young values lost from old arrays: ${bad}`);
