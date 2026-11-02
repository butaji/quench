function write(object, value) {
  object.left = value;
  return value;
}

function strictWrite(object, value) {
  'use strict';
  object.left = value;
  return value;
}

function updateAlias(object, value) {
  const alias = object;
  const before = alias.left;
  object.left = value;
  return before + alias.left + object.right;
}

function invoke(object) { return object.left(); }
function replacement() { return 2; }

function append(queue) {
  this.left = null;
  if (queue === null) return this;
  let next = queue;
  while (next.left !== null) next = next.left;
  next.left = this;
  return queue;
}

globalThis.packed = { left: 1, right: 2 };
globalThis.arena = { left: 1.5, right: 2.5 };
globalThis.effects = 0;
globalThis.accessor = { set left(value) { effects += value; }, right: 2 };
globalThis.readonly = Object.freeze({ left: 1, right: 2 });
globalThis.proxy = new Proxy({ left: 1, right: 2 }, {
  set(target, key, value) { effects++; target[key] = value; return true; }
});
globalThis.methodOwner = { left() { return 1; }, right: 2 };
globalThis.head = { left: { left: null, right: 10 }, right: 20 };
globalThis.item = { left: 9, right: 30 };
globalThis.arenaHead = { left: { left: null, right: 10.5 }, right: 20.5 };
globalThis.arenaItem = { left: 9, right: 30.5 };
for (let index = 0; index < 100; index++) {
  write(packed, 1);
  write(arena, 1.5);
  strictWrite(packed, 1);
  strictWrite(arena, 1.5);
  updateAlias(packed, 1);
  updateAlias(arena, 1.5);
  append.call({ left: 9, right: 3 }, { left: { left: null, right: 4 }, right: 5 });
}
