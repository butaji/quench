const a = new Array(16900);
const before = {
  length: a.length,
  ownKeys: Object.keys(a).length,
  hasZero: 0 in a,
};
a[16899] = "tail";
a[1] = undefined;
let mapCalls = 0;
a.map(() => mapCalls++);
const afterFill = {
  length: a.length,
  ownKeys: Object.keys(a).length,
  hasOne: 1 in a,
  hasLast: 16899 in a,
  mapCalls,
};
a.length = 8;
a.length = 16900;
const afterShrinkGrow = {
  length: a.length,
  hasOne: 1 in a,
  hasLast: 16899 in a,
  ownKeys: Object.keys(a).length,
};

const b = new Array(1_000_000);
b[3] = "kept";
b[999_999] = "dropped";
b.length = 16_900;
const afterSparseShrink = {
  length: b.length,
  hasThree: 3 in b,
  three: b[3],
  hasLast: 16_899 in b,
  ownKeys: Object.keys(b).length,
};

console.log(JSON.stringify({ before, afterFill, afterShrinkGrow, afterSparseShrink }));
