function readIndex(array, index) {
  return array[index];
}

const dense = [3, 5, 8];
for (let index = 0; index < 32768; index++) readIndex(dense, index % dense.length);

const sparse = [];
sparse.length = 3;
Object.setPrototypeOf(sparse, { 1: "inherited" });
console.log(JSON.stringify({
  dense: [readIndex(dense, 0), readIndex(dense, 1), readIndex(dense, 2)],
  holeReadsPrototype: readIndex(sparse, 1),
  missing: readIndex(sparse, 8) === undefined ? "undefined" : "unexpected",
}));
