function make(value) {
        function store() { 'use strict'; this.value = value; }
        return { store, set(next) { value = next; }, value: 0, a: 1, b: 2 };
      }
      function exercise() {
        const target = make(0);
        for (let index = 0; index < 128; index++) {
          target.set(index);
          if (target.store() !== undefined || target.value !== index) throw 'numeric store';
        }
        for (const value of [-0, 1.5, NaN, Infinity, 4n, {}]) {
          target.set(value);
          target.store();
          if (!Object.is(target.value, value)) throw 'capture identity';
        }
        let effects = 0;
        target.set(7);
        Object.defineProperty(target, 'value', { configurable: true, set(value) {
          if (value !== 7) throw 'setter value'; effects++;
        } });
        target.store();
        if (effects !== 1) throw 'setter ordering';
        Object.defineProperty(target, 'value', { configurable: true, writable: true, value: 8 });
        const proxy = new Proxy(target, { set(object, key, value) { effects++; object[key] = value; return true; } });
        proxy.store();
        if (effects !== 2 || target.value !== 7) throw 'proxy store';
        for (let index = 0; index < 64; index++) target.store();
        Object.freeze(target);
        target.set(9);
        let threw = false;
        try { target.store(); } catch (error) { threw = error instanceof TypeError; }
        if (!threw || target.value !== 7) throw 'frozen strict fallback';
      }
      exercise();
      function makeArrow(value) {
        const owner = { value: 0, a: 1, b: 2 };
        owner.store = (function () { return () => { this.value = value; }; }).call(owner);
        return owner;
      }
      function exerciseKinds() {
        const owner = makeArrow(17);
        const borrowed = { value: 0, a: 1, b: 2, store: owner.store };
        for (let index = 0; index < 64; index++) borrowed.store();
        if (owner.value !== 17 || borrowed.value !== 0) throw 'arrow receiver';
        function makeClass(value) {
          return class CapturedClass { constructor() { this.value = value; } };
        }
        const target = { value: 0, a: 1, b: 2, constructor: makeClass(19) };
        let threw = false;
        try { target.constructor(); } catch (error) { threw = error instanceof TypeError; }
        if (!threw || target.value !== 0) throw 'class call fallback';
      }
      exerciseKinds();

function makeUpdates(mask) {
  return { value: 0, a: 1, b: 2, setMask(next) { mask = next; },
    and() { 'use strict'; this.value = this.value & mask; },
    or() { 'use strict'; this.value = this.value | mask; },
    xor() { 'use strict'; this.value = this.value ^ mask; },
    shl() { 'use strict'; this.value = this.value << mask; },
    shr() { 'use strict'; this.value = this.value >> mask; },
    ushr() { 'use strict'; this.value = this.value >>> mask; }
  };
}
function exerciseUpdates() {
  const target = makeUpdates(2);
  for (const [method, initial, expected] of [
    ['and', 1, 0], ['or', 1, 3], ['xor', 1, 3],
    ['shl', 1, 4], ['shr', -8, -2], ['ushr', -8, 1073741822]
  ]) {
    for (let index = 0; index < 128; index++) {
      target.value = initial;
      if (target[method]() !== undefined || target.value !== expected) throw 'bitwise store';
    }
  }
  for (const [mask, expected] of [[-0, 1], [1.5, 1], [4294967297, 1], [NaN, 1], [Infinity, 1], [-2147483649, 2147483647]]) {
    target.setMask(mask); target.value = 1; target.or();
    if (target.value !== expected) throw 'mask conversion';
  }
  target.setMask(2); target.value = 1.5; target.or();
  if (target.value !== 3) throw 'field conversion';
  for (const [mask, expected] of [[0, 4294967295], [32, 4294967295], [-1, 1]]) {
    target.setMask(mask); target.value = -1; target.ushr();
    if (target.value !== expected) throw 'unsigned result';
  }
  target.setMask(2n); target.value = 1;
  let mixedThrew = false;
  try { target.or(); } catch (error) { mixedThrew = error instanceof TypeError; }
  if (!mixedThrew || target.value !== 1) throw 'mixed bigint';
  target.value = 1n; target.or();
  if (target.value !== 3n) throw 'bigint store';
  let effects = '';
  target.setMask(2);
  target.value = { valueOf() { effects += 'v'; target.setMask(8); return 1; } };
  target.or();
  if (target.value !== 3 || effects !== 'v') throw 'coercion capture ordering';
  let assigned;
  target.setMask(2);
  Object.defineProperty(target, 'value', { configurable: true,
    get() { effects += 'g'; target.setMask(4); return 1; },
    set(value) { effects += 's'; assigned = value; }
  });
  target.or();
  if (assigned !== 5 || effects !== 'vgs') throw 'accessor capture ordering';
  const frozen = makeUpdates(2);
  for (let index = 0; index < 64; index++) frozen.or();
  Object.freeze(frozen); frozen.setMask(4);
  let frozenThrew = false;
  try { frozen.or(); } catch (error) { frozenThrew = error instanceof TypeError; }
  if (!frozenThrew || frozen.value !== 2) throw 'frozen update';
}
exerciseUpdates();
