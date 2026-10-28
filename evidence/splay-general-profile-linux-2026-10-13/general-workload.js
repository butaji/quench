class Record {
  constructor(id) {
    this.id = id;
    this.left = null;
    this.right = null;
    this.payload = [id, id + 1, id + 2];
    this.active = true;
  }
}

function mix(record, step) {
  record.payload[step % 3] = (record.payload[step % 3] + step) | 0;
  if ((step & 7) === 0) record.active = !record.active;
  return record.payload[step % 3] + (record.active ? record.id : -record.id);
}

function visit(record, depth, step) {
  if (record === null || depth === 0) return 0;
  return mix(record, step) + visit(record.left, depth - 1, step + 1) + visit(record.right, depth - 1, step + 2);
}

function makeTree(depth, first) {
  if (depth === 0) return null;
  const root = new Record(first);
  root.left = makeTree(depth - 1, first * 2);
  root.right = makeTree(depth - 1, first * 2 + 1);
  return root;
}

const roots = [];
for (let round = 0; round < 40; round++) {
  const root = makeTree(7, round + 1);
  roots.push(root);
  let total = 0;
  for (let step = 0; step < 80; step++) total += visit(root, 7, step);
  if (total === Number.MIN_SAFE_INTEGER) throw new Error("unreachable");
}
console.log(roots.length);
