// Self-hosted Object static methods over the `__ops__` bridge.

// Object.is(a, b) — §20.1.2.10: SameValue (NaN equals NaN, +0 !== -0).
Object.is = function is(a, b) {
  return __ops__.sameValue(a, b);
};

// Object.isExtensible(o) — §19.1.2.11: IsExtensible.
Object.isExtensible = function isExtensible(o) {
  return __ops__.isExtensible(o);
};