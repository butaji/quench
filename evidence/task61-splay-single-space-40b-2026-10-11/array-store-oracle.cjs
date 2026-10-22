function sloppySet(array, index, value) {
  array[index] = value;
  return array[index];
}

function strictSet(array, index, value) {
  "use strict";
  array[index] = value;
  return array[index];
}

const writable = [10, 20, 30];
Object.defineProperty(writable, "1", {
  value: 20,
  writable: true,
  enumerable: false,
  configurable: true,
});
const descriptorWrite = sloppySet(writable, 1, 21);

const readonly = [10, 20];
Object.defineProperty(readonly, "1", { value: 20, writable: false });
const reflectReadonly = Reflect.set(readonly, "1", 21);
let strictReadonlyError = null;
try {
  strictSet(readonly, 1, 22);
} catch (error) {
  strictReadonlyError = error.name;
}

const frozen = Object.freeze([10, 20]);
const reflectFrozen = Reflect.set(frozen, "0", 11);
const sloppyFrozen = sloppySet(frozen, 0, 12);
let strictFrozenError = null;
try {
  strictSet(frozen, 0, 13);
} catch (error) {
  strictFrozenError = error.name;
}

let inheritedSetterCalls = 0;
Object.defineProperty(Array.prototype, "8", {
  set(value) {
    inheritedSetterCalls += value;
  },
  configurable: true,
});
const inheritedSetterArray = [];
inheritedSetterArray.length = 8;
inheritedSetterArray[8] = 5;
const inheritedSetterResult = {
  calls: inheritedSetterCalls,
  length: inheritedSetterArray.length,
  own: Object.prototype.hasOwnProperty.call(inheritedSetterArray, "8"),
};
delete Array.prototype[8];

Object.defineProperty(Array.prototype, "9", {
  value: 90,
  writable: false,
  configurable: true,
});
const inheritedReadonlyArray = [];
inheritedReadonlyArray.length = 9;
const inheritedReadonlyResult = {
  set: Reflect.set(inheritedReadonlyArray, "9", 99),
  length: inheritedReadonlyArray.length,
  own: Object.prototype.hasOwnProperty.call(inheritedReadonlyArray, "9"),
};
delete Array.prototype[9];

const sealed = [10];
Object.preventExtensions(sealed);
const reflectNewIndex = Reflect.set(sealed, "1", 11);
const sloppyNewIndex = sloppySet(sealed, 1, 12);
let strictNewIndexError = null;
try {
  strictSet(sealed, 1, 13);
} catch (error) {
  strictNewIndexError = error.name;
}

console.log(JSON.stringify({
  descriptorWrite,
  writable,
  readonly: { reflect: reflectReadonly, value: readonly[1], strictError: strictReadonlyError },
  frozen: { reflect: reflectFrozen, sloppyValue: sloppyFrozen, value: frozen[0], strictError: strictFrozenError },
  inheritedSetterResult,
  inheritedReadonlyResult,
  nonExtensible: {
    reflect: reflectNewIndex,
    sloppyValue: sloppyNewIndex,
    length: sealed.length,
    strictError: strictNewIndexError,
  },
}));
