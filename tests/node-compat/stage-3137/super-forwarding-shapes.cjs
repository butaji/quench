'use strict';
// Constructors of classes extending natives: those that only forward to the parent and
// near misses whose bodies are longer, reordered or drop values, constructed often enough
// for every call path to take part.
class Implicit extends Map {}
class Forwarded extends Array {
  constructor(a, b) { super(a, b); }
}
class Swapped extends Array {
  constructor(a, b) { super(b, a); }
}
class Dropped extends Array {
  constructor(a, b) { super(a); }
}
class Longer extends Map {
  constructor(entries) { super(entries); this.tag = 'longer'; }
}
class Guarded extends Set {
  constructor(values) { super(values); if (values === undefined) this.empty = true; }
}

const rows = [];
for (let round = 0; round < 2000; round++) {
  const implicit = new Implicit([[round, 'v']]);
  const forwarded = new Forwarded(round, round + 1);
  const short = new Forwarded(round);
  const swapped = new Swapped(1, 2);
  const dropped = new Dropped(3, 4);
  const longer = new Longer([[1, round]]);
  const guarded = new Guarded();
  if (round % 500 === 0) {
    rows.push([
      implicit.get(round), implicit instanceof Implicit, Object.getPrototypeOf(implicit) === Implicit.prototype,
      forwarded.length, forwarded.join(), forwarded instanceof Forwarded,
      short.length, Array.isArray(short),
      swapped.join(), dropped.join(), dropped.length,
      longer.get(1), longer.tag, Object.keys(longer),
      guarded.size, guarded.empty, Object.getPrototypeOf(guarded) === Guarded.prototype,
    ]);
  }
}
console.log(JSON.stringify(rows));
// The implicit constructor passes a single length on, so the parent's own check raises.
class Sized extends Array {}
try {
  new Sized(-1);
} catch (error) {
  console.log(error.constructor.name, error.message);
}
