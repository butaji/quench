function readPair(node) {
  const first = node.left;
  const alias = node;
  return first + alias.right;
}

function update(value) {
  const before = this.left;
  const alias = this;
  this.left = value;
  return before + alias.left + this.right;
}

function reassigned(node, other) {
  const first = node.left;
  node = other;
  return first + node.right;
}

function joined(node, other, flag) {
  let active = node;
  if (flag) active = other;
  return active.left + active.right;
}

globalThis.packed = { left: 1, right: 2 };
globalThis.arena = { left: 1.5, right: 2.5 };
globalThis.wider = { left: 10, right: 20, extra: 30 };
globalThis.effects = 0;
globalThis.getter = { get left() { effects++; return 11; }, right: 12 };
globalThis.proxy = new Proxy({ left: 13, right: 14 }, {
  get(target, key) { effects++; return target[key]; }
});
globalThis.extras = { left: 4, right: 6 };
Object.setPrototypeOf(extras, { inherited: 1 });
globalThis.child = Object.create(extras);
for (let index = 0; index < 100; index++) {
  readPair(packed);
  readPair(arena);
  update.call(packed, 1);
  update.call(arena, 1.5);
  reassigned(packed, wider);
  reassigned(arena, wider);
  joined(packed, arena, index & 1);
}
