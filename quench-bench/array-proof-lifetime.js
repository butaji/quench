// Guest oracle: shared literal reads interleaved with clones, writes and holes.
function literal() { return [1, 2, 3, 4]; }
const retained = [];
for (let i = 0; i < 128; i++) retained.push(literal());
let sum = 0;
for (let pass = 0; pass < 8; pass++) {
  for (let i = 0; i < retained.length; i++) {
    const current = retained[i];
    sum += current[0] + current[3];
    const sibling = literal();
    current[0] = pass + i;
    if (sibling[0] !== 1 || sibling[1] !== 2) throw new Error('shared write leaked');
    if (pass === 3) delete current[2];
    if (pass === 4) current[2] = i;
    sum += current[2] === undefined ? 0 : current[2];
    if (current[0] !== pass + i) throw new Error('stale array read');
  }
}
const untouched = literal();
if (untouched.join(',') !== '1,2,3,4') throw new Error('literal backing changed');
console.log(JSON.stringify([sum, retained[0], retained[retained.length - 1], untouched]));
